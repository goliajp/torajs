//! Terminal direct-call emit pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` arm as
//! chunk-84a of the decomp. The 37+ specialized `try_lower`
//! sibling cascade (see [`crate::ssa_lower::lower_expr_inner`])
//! claims its case-shape; whatever falls through lands here and
//! emits the generic SSA `Call` instruction.
//!
//! Pipeline:
//!
//! 1. **M3 generic call retarget** — `call_retargets[eid]` (recorded
//!    by the typechecker for monomorphized generics) overrides
//!    `resolve_callee`'s ident lookup.
//! 2. **Math.sumPrecise dispatch swap** — default `math_sum_precise`
//!    is the F64-array helper; swap to `math_sum_precise_i64` when
//!    the single arg is statically `Type::Arr<I64>` (literal-int
//!    sources lower that way).
//! 3. **T-28 trailing-Any pad** — `arity_pad_count[eid]` (when all
//!    trailing missing params were `Type::Any`) → push one
//!    ANY_UNDEF Any-box per missing slot to align argv with the
//!    callee's static signature.
//! 4. **Arg ownership = SHARE** — non-Copy args pass as +0 borrows
//!    and are never consumed (the consuming-params bitmap retired
//!    in chunk 568: every store lane takes its own +1 per chunks
//!    564-567, so caller-side consume double-counted into a leak);
//!    owned-shape temps release after the call.
//! 5. **Argument coercion** — Math.* unary/binary takes `f64` (with
//!    `Any → any_to_number` for S342); generic fn_sigs walk widens
//!    `I64/Bool → F64`, truncates `F64/Bool → I64` (matches JS
//!    `ToInt32` / `ToUint32` prefix for indexes / codepoints / bit
//!    positions); `Any` param + concrete arg routes through
//!    `box_to_any_from_expr` (P0.9 + S126-3 — `undefined`/`null`
//!    literals keep `ANY_UNDEF`/`ANY_NULL` tags instead of
//!    collapsing to ConstPtrNull).
//! 6. **Emit `Call`** with `f_ret_type_hint(target)` + `emit_throw_check`.

mod pad;
mod str_split;

pub(crate) use pad::{pad_trailing_undef, pad_undef_n};

use crate::ast::ExprId;
use crate::ssa::{BinOp, FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_terminal_coerce::coerce_args;
use crate::ssa_lower_call_terminal_headless::{
    factory_relay_argv, headless_slots, insert_headless_slots, pack_headless_argv,
};
use crate::ssa_lower_object_lit::declared::param_ann;

pub(crate) fn emit(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    callee: ExprId,
    args: &[ExprId],
) -> Operand {
    let mut target = resolve_target(ctx, eid, callee);
    // RFC 20260810-indirect-argc-abi H1 — a head-less T-31 callee's
    // sig carries the hidden I64 `__torajs_argc` at position 0. The
    // AST face (and so `args`) never sees it: every param-type read
    // below shifts past the slot, and the argc operand is prepended
    // only at the very end, right before the `Call` emit — inserting
    // earlier would shift the pad/coerce/dflt-lit zips off by one.
    let headless = headless_slots(ctx, eid, callee);
    let hidden_off = headless.width();
    // Chunk 641 — an empty `[]` arg allocs with the PARAM's layout
    // (checker admit `empty_lit_into_arr` pairs here); every other
    // arg lowers plain.
    let param_tys: Option<Vec<Type>> = ctx
        .fn_sig_ids
        .get(&target)
        .map(|sid| ctx.fn_sigs[sid.0 as usize].0.clone());
    // Rotation 549 — an owned-shape arg temp is alive across every
    // later arg's lower and the callee itself (args are +0 shares,
    // released after the call): park each one so a throw in between
    // drops it (`f({} as any, boom())` stranded the object per caught
    // throw, 175MB over 600k). Unparked after the post-call check.
    let mut arg_toks: Vec<usize> = Vec::new();
    let mut argv: Vec<Operand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let v = 'arg: {
                let expected = param_tys.as_ref().and_then(|ps| ps.get(headless.sig_at(i)));
                if let Some(op) = ctx.try_lower_empty_array_arg(*a, expected) {
                    break 'arg op;
                }
                // 2026-07-16 — mirror stmt_let_decl.rs:411 (`ty == Any &&
                // ObjectLit → lower_dynobj_init`): a direct ObjectLit arg
                // into an `any`-typed param goes straight through the
                // dobj lane so `undefined` field values keep the
                // `ANY_UNDEF` tag (`lower_dynobj_init`'s `Ident("undefined")`
                // special case, ssa_lower_dynobj_init.rs:83). Without this,
                // the arg lowered as a `Type::Struct` value and the
                // subsequent `box_to_any_from_expr` at the coerce step
                // collapsed the Type::Undefined slot to ptr-null → tag 0,
                // so `verifyProperty(newObj, "prop", { value: undefined,
                // ... })`'s `desc.value` read back as `null` (blocking the
                // test262 "descriptor value should be null" 8-case cluster
                // under `Object/create`).
                if let Some(Type::Any) = expected
                    && matches!(ctx.ast.get_expr(*a), crate::ast::Expr::ObjectLit { .. })
                {
                    // ANY_HEAP encode (mirrors the as-cast / let-decl
                    // promotes). The raw Ptr face leaked every arg cell:
                    // `release_owned_temp` keys the drop off the operand
                    // type, and `is_copy(Ptr)` = no release site — 300k
                    // `take({a:i})` churned 83.5MB vs 6.4MB flat. The
                    // Any box gives the release a type it drops AND the
                    // callee a face that passes the any-lane tag gates.
                    let dynobj = ctx.lower_dynobj_init(*a);
                    break 'arg ctx.box_to_any(dynobj);
                }
                // Chunk 784 — pin the param's declared struct layout for
                // a direct ObjectLit arg (mirrors the chunk-780 let-decl
                // site): without it resolve_objlit_layout first-matches a
                // same-shaped layout registered under a different
                // declared type and the slot reprs collide (silent-wrong
                // reads through the declared layout).
                if let Some(&slot) = expected
                    && ctx.pins_declared(slot, *a)
                {
                    let ann = param_ann(ctx, (callee, target), i);
                    ctx.pin_declared(slot, ann.as_deref(), *a);
                }
                let v = ctx.lower_expr(*a);
                ctx.let_declared_obj_layout = None;
                v
            };
            if let Some((op, ty)) = ctx.throw_temp_of(*a, &v) {
                arg_toks.push(ctx.push_throw_temp(op, ty));
            }
            v
        })
        .collect();
    // a Nullable<Array> arg decayed against a generic Array param at
    // the checker (generic_ident decay_nullable_arr): guard the runtime
    // null before it enters the monomorphized callee as a bare Arr.
    // Only retargeted (generic) call sites see the decay, and only a
    // bare-Arr param slot qualifies: a Null⊔Nullable typevar join
    // instantiates at Any, whose boxed lane carries the null legally.
    if ctx.call_retargets.contains_key(&eid) {
        for (i, (a, op)) in args.iter().zip(argv.clone().iter()).enumerate() {
            let param_is_arr = param_tys
                .as_ref()
                .and_then(|ps| ps.get(headless.sig_at(i)))
                .is_some_and(|t| matches!(t, Type::Arr(_)));
            if param_is_arr {
                crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, *a, op);
            }
        }
    }
    crate::ssa_lower_nullable_guard::emit_entry_guards(ctx, eid, args, &argv);
    // RFC 20260705 chunk 548 — snapshot owned-shape arg temps
    // (`f(g())` nested-call results) before pad/coerce mutate argv;
    // they are released after the call.
    let owned_temps: Vec<(usize, Operand)> = args
        .iter()
        .zip(argv.iter())
        .enumerate()
        .filter(|(_, (a, _))| ctx.expr_owned_shape(**a))
        .map(|(i, (_, op))| (i, *op))
        .collect();
    // RFC 20260816-headless-argv-face — pack the raw argv buffer
    // BEFORE the truncate/coerce steps below: those align the SSA
    // arg list down to the callee's declared signature, while the
    // arguments object must see every value the source passed
    // (`f(1, 2, 3)` into `function f(a) { … arguments[2] … }`).
    // Packing here also reuses the operands already lowered above —
    // re-lowering the arg expressions would double their side
    // effects.
    let headless_argv = headless.argv.then(|| {
        factory_relay_argv(ctx, callee)
            .unwrap_or_else(|| pack_headless_argv(ctx, eid, args, &argv, headless.at))
    });
    // Direct-call trailing-ignore (checker wedge mirror): the extra
    // args lowered above for §13.3.6.1 side effects; the callee's ABI
    // reads only its declared slots, so argv aligns down to the
    // signature. Owned-shape extras were snapshotted with the rest
    // and release after the call.
    if let Some(ps) = &param_tys
        && argv.len() > ps.len() - hidden_off
    {
        argv.truncate(ps.len() - hidden_off);
    }
    target = maybe_swap_math_sum_precise(ctx, target, &argv);
    pad_trailing_undef(ctx, eid, &mut argv);
    let coerce_owned = coerce_args(ctx, target, headless, args, &mut argv);
    // The coerce-minted temps live across the callee too.
    let coerce_toks: Vec<usize> = coerce_owned
        .iter()
        .map(|(op, ty)| ctx.push_throw_temp(*op, *ty))
        .collect();
    // H1 — the synthetic slots move in LAST, after every positional
    // zip above has run against the user-aligned argv.
    insert_headless_slots(ctx, eid, callee, headless, args, &mut argv, headless_argv);
    let result = match str_split::emit(ctx, eid, target, &argv) {
        Some(r) => r,
        None => emit_direct_call(ctx, target, argv),
    };
    ctx.emit_throw_check(Some(target));
    // Unpark before the unconditional releases below.
    for t in arg_toks.into_iter().chain(coerce_toks) {
        ctx.pop_throw_temp(t);
    }
    for (i, op) in owned_temps {
        ctx.release_owned_temp(args[i], &op);
    }
    // chunk 2a — fresh-owned Any→Str conversions minted by
    // coerce_args carry their own +1; the callee borrowed them, so
    // release here.
    for (op, ty) in coerce_owned {
        ctx.emit_drop_value(op, ty);
    }
    result
}

/// The `Call` of a direct call, answering its value in the callee's
/// return type.
fn emit_direct_call(ctx: &mut LowerCtx<'_>, target: FuncId, argv: Vec<Operand>) -> Operand {
    // Math.imul inline: coerce_args already put both operands through
    // ToInt32 (sign-extended int32 in i64), so their i64 product cannot
    // overflow and its low 32 bits are the int32 product; sign-extending
    // them answers what the kernel answers, without the call
    if target == ctx.intrinsics.math_imul {
        debug_assert_eq!(argv.len(), 2, "Math.imul takes 2 coerced args");
        let p = ctx.bin(BinOp::Mul, argv[0], argv[1], Type::I64);
        return ctx.emit_to_int32(p);
    }
    let ret_ty = ctx.f_ret_type_hint(target);
    let cur_block = ctx.cur_block;
    // Chunk 806 — a void callee has no return value; consuming the
    // Call's SSA result read x0 garbage (`console.log(v("x"))`
    // printed a residue integer). Emit for effect and answer
    // ConstPtrNull — the payload every Undefined-typed value
    // carries (the checker types this call Undefined), so the
    // Undefined-aware consumers (box_to_any_from_expr's Ptr arm,
    // strict-eq, let-init) see a real undefined. The indirect
    // lanes answer the same.
    if ret_ty == Type::Void {
        ctx.f.append_void(cur_block, InstKind::Call(target, argv));
        Operand::ConstPtrNull
    } else {
        let v = ctx
            .f
            .append_inst(cur_block, InstKind::Call(target, argv), ret_ty, None);
        Operand::Value(v)
    }
}

fn resolve_target(ctx: &LowerCtx<'_>, eid: ExprId, callee: ExprId) -> FuncId {
    // M3 — generic call retarget. If the typechecker recorded a
    // (mono fn name) for this call's ExprId, look up the specialized
    // FuncId by name and use it instead of the generic ident's
    // resolve.
    if let Some(mono_name) = ctx.call_retargets.get(&eid).cloned() {
        *ctx.fn_table.get(&mono_name).unwrap_or_else(|| {
            panic!("ssa-lower: monomorphized fn `{mono_name}` missing from fn_table")
        })
    } else {
        ctx.resolve_callee(callee)
    }
}

fn maybe_swap_math_sum_precise(ctx: &LowerCtx<'_>, target: FuncId, argv: &[Operand]) -> FuncId {
    if target == ctx.intrinsics.math_sum_precise
        && argv.len() == 1
        && let Type::Arr(arr_id) = ctx.operand_ty(&argv[0])
    {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if matches!(elem_ty, Type::I64) {
            return ctx.intrinsics.math_sum_precise_i64;
        }
    }
    target
}
