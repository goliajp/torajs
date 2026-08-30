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

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_call_terminal_headless::{
    forwarded_argc, headless_slots, pack_headless_argv, user_argc,
};

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
    let mut argv: Vec<Operand> = args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let expected = param_tys.as_ref().and_then(|ps| ps.get(i + hidden_off));
            if let Some(op) = ctx.try_lower_empty_array_arg(*a, expected) {
                return op;
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
                return ctx.box_to_any(dynobj);
            }
            // Chunk 784 — pin the param's declared struct layout for
            // a direct ObjectLit arg (mirrors the chunk-780 let-decl
            // site): without it resolve_objlit_layout first-matches a
            // same-shaped layout registered under a different
            // declared type and the slot reprs collide (silent-wrong
            // reads through the declared layout).
            if let Some(Type::Obj(sid)) = expected
                && matches!(ctx.ast.get_expr(*a), crate::ast::Expr::ObjectLit { .. })
            {
                ctx.let_declared_obj_layout = Some(*sid);
            }
            let v = ctx.lower_expr(*a);
            ctx.let_declared_obj_layout = None;
            v
        })
        .collect();
    // RC-4 — a Nullable<Array> arg decayed against a generic Array
    // param at the checker (generic_ident decay_nullable_arr); guard
    // the runtime null before it enters the monomorphized callee as
    // a bare Arr. Only retargeted (generic) call sites qualify —
    // non-generic calls never see the decay. Cluster #6 sibling
    // (rotation 442) — only a bare-Arr param slot qualifies: a
    // Null⊔Nullable typevar join now instantiates at Any, whose
    // boxed lane carries the null legally, so guarding by source
    // shape alone would throw on an argument the checker admitted.
    if ctx.call_retargets.contains_key(&eid) {
        for (i, (a, op)) in args.iter().zip(argv.clone().iter()).enumerate() {
            let param_is_arr = param_tys
                .as_ref()
                .and_then(|ps| ps.get(i + hidden_off))
                .is_some_and(|t| matches!(t, Type::Arr(_)));
            if param_is_arr {
                crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, *a, op);
            }
        }
    }
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
    let headless_argv_op = headless
        .argv
        .then(|| pack_headless_argv(ctx, eid, args, &argv));
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
    let coerce_owned = coerce_args(ctx, target, hidden_off, args, &mut argv);
    // H1 — prepend the argc operand LAST, after every positional zip
    // above has run against the user-aligned argv. The count is the
    // call's argument-expression count (during the H1 double-feed
    // the AST-prepended argc arg rides in it — the slot has no
    // readers until H2 retires that prepend, which also removes the
    // extra arg and makes this count the true user argc).
    if let Some(op) = headless_argv_op {
        argv.insert(0, op);
    }
    if headless.argc {
        // …minus the trailing slots `apply_default_args` materialized
        // from the callee's declared defaults: those are the callee's
        // own values, not arguments the program passed.
        let argc_op =
            forwarded_argc(ctx, callee)
                .unwrap_or(Operand::ConstI64(user_argc(ctx, eid, args.len()) as i64));
        argv.insert(0, argc_op);
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
    let result = if ret_ty == Type::Void {
        ctx.f.append_void(cur_block, InstKind::Call(target, argv));
        Operand::ConstPtrNull
    } else {
        let v = ctx
            .f
            .append_inst(cur_block, InstKind::Call(target, argv), ret_ty, None);
        Operand::Value(v)
    };
    ctx.emit_throw_check(Some(target));
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

pub(crate) fn pad_trailing_undef(ctx: &mut LowerCtx<'_>, eid: ExprId, argv: &mut Vec<Operand>) {
    // T-28 — pad trailing missing Type::Any params with ANY_UNDEF
    // Any-box operands. check.rs's arity_pad_count recorded the
    // missing count for this Call ExprId iff all the trailing
    // missing params were Type::Any (the typed-tier strict-arity
    // check rejected mixed-typed missing). Shared by the indirect
    // arms (closure_local / fn_indirect) — an indirect CallIndirect
    // whose argv is shorter than its signature reads garbage
    // registers in the callee (RC-4 arguments-object SIGSEGV).
    let pad_n = match ctx.arity_pad_count.get(&eid).copied() {
        Some(n) if n > 0 => n,
        _ => return,
    };
    pad_undef_n(ctx, pad_n, argv);
}

/// Emit `n` ANY_UNDEF boxes onto `argv` — the count-known half of
/// [`pad_trailing_undef`], for arms that resolve the pad count from
/// `arity_pad_count` themselves before dropping the call ExprId
/// (struct-method dispatch resolves it in `try_lower`; its emit
/// helpers are shared with fixed-arity accessor callers that pad 0).
pub(crate) fn pad_undef_n(ctx: &mut LowerCtx<'_>, n: usize, argv: &mut Vec<Operand>) {
    for _ in 0..n {
        let cur_block = ctx.cur_block;
        let undef_box = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        argv.push(Operand::Value(undef_box));
    }
}

/// Clean the F64 `undefined` sentinel out of a Math argument. The
/// arg ExprId is absent only for synthesized calls, which never
/// carry a sentinel.
fn canon_math_arg(ctx: &mut LowerCtx<'_>, v: Operand, arg: Option<ExprId>) -> Operand {
    if ctx.operand_ty(&v) == Type::F64
        && arg.is_some_and(|e| crate::ssa_lower_nullable_guard::is_undef_f64_source(ctx, e))
    {
        return ctx.canon_f64_away_from_sentinel(v);
    }
    v
}

fn coerce_args(
    ctx: &mut LowerCtx<'_>,
    target: FuncId,
    hidden_off: usize,
    args: &[ExprId],
    argv: &mut [Operand],
) -> Vec<(Operand, Type)> {
    // ToNumber(undefined) is NaN — the F64 `undefined` sentinel must
    // not reach a Math kernel, which would hand its payload back out
    // (`Math.abs` clears the sign bit and returns the pattern
    // unchanged). See [`crate::ssa_lower_f64_sentinel_canon`].
    if ctx.is_math_unary(target) {
        debug_assert_eq!(argv.len(), 1, "Math.* unary takes 1 arg");
        argv[0] = coerce_to_f64_or_any_to_number(ctx, argv[0]);
        argv[0] = canon_math_arg(ctx, argv[0].clone(), args.first().copied());
        return Vec::new();
    }
    if ctx.is_math_binary(target) {
        debug_assert_eq!(argv.len(), 2, "Math.* binary takes 2 args");
        for i in 0..2 {
            argv[i] = coerce_to_f64_or_any_to_number(ctx, argv[i]);
            argv[i] = canon_math_arg(ctx, argv[i].clone(), args.get(i).copied());
        }
        return Vec::new();
    }
    let Some(sig_id) = ctx.fn_sig_ids.get(&target).copied() else {
        return Vec::new();
    };
    // H1 — cut the head-less hidden slot off both sig-aligned lists
    // (param types here, dflt lits inside apply_runtime_dflt_lits)
    // so every zip below stays user-aligned; the argc operand is
    // prepended after coercion, in `emit`.
    let param_tys = ctx.fn_sigs[sig_id.0 as usize].0[hidden_off..].to_vec();
    apply_runtime_dflt_lits(ctx, target, hidden_off, &param_tys, argv);
    coerce_args_by_param_tys(ctx, &param_tys, args, argv)
}

/// S3.8 — a runtime `undefined` actual binds a typed param's
/// Number/Bool/short-string literal default (§10.2.11: explicit
/// undefined and missing bind alike). The call-site pad handles the static
/// spellings; an `any`-typed actual can only be tested at runtime,
/// so it routes through the same branchless or_default kernel the
/// boxed dual-entry adapter uses, ahead of the ordinary Any→typed
/// coercion (which would have answered NaN / false). Positions
/// whose param is itself `Any` never qualify: their default is the
/// body-side IsUndefined guard (`materialize_expr_defaults`), and
/// Pass 1 sees the rewritten `undefined` literal there — no entry.
fn apply_runtime_dflt_lits(
    ctx: &mut LowerCtx<'_>,
    target: FuncId,
    hidden_off: usize,
    param_tys: &[Type],
    argv: &mut [Operand],
) {
    let Some(lits) = ctx.fn_dflt_lits.get(&target) else {
        return;
    };
    // The lits table is sig-aligned; `param_tys` arrives with the
    // head-less hidden slot already cut, so cut the same prefix here
    // to keep the zip user-aligned (H1).
    let lits = lits[hidden_off.min(lits.len())..].to_vec();
    for (i, expected) in param_tys.iter().enumerate() {
        if i >= argv.len() {
            break;
        }
        let Some(Some(lit)) = lits.get(i) else {
            continue;
        };
        if ctx.operand_ty(&argv[i]) != Type::Any
            || !matches!(
                expected,
                Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str
            )
        {
            continue;
        }
        // A long-string literal has no compile-time box — intern it
        // as a static `.rodata` Str global (rc no-op) and pass the
        // pointer bits under the pair protocol's Heap tag. The
        // "ownership transfer" the tag-4 doc names is vacuous for a
        // FLAG_STATIC_LITERAL global, and the downstream AnyToStr
        // coercion mints its own owned copy either way.
        let (tag, bits) = match lit {
            crate::ssa_lower_boxed_entry::DfltLit::StrLong(s) => {
                let ptr = ctx.intern_string_literal(s);
                let p = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::PtrToInt(Operand::Value(ptr)),
                    Type::I64,
                    None,
                );
                (4i64, Operand::Value(p))
            }
            lit => lit
                .box_encoding()
                .expect("non-StrLong DfltLit always has a compile-time box"),
        };
        argv[i] = Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.anyv_or_default,
                vec![argv[i], Operand::ConstI64(tag), bits],
            ),
            Type::Any,
            None,
        ));
    }
}

/// Per-arg conversion against a declared param-type list, applying the
/// shared contract (`ssa_lower_call_arg_conv`) to each position.
///
/// Shared by the direct-call terminal and the struct-field closure
/// dispatch (RFC 20260714-t262-top-clusters 刀 1 — an Array-literal
/// arg into an obj-method's any param used to pass its typed repr
/// raw). Returns the fresh-owned temps the caller must release after
/// the call, each with the type to release it as.
///
/// `args` and `argv` line up positionally: callers with leading slots
/// of their own (an env pointer, a receiver) pass the slice of `argv`
/// that holds exactly these args.
///
/// `argv` may still run past `args` at the tail — the direct-call
/// terminal pads missing trailing Any params with synthesized
/// `ANY_UNDEF` boxes *before* calling here. Those positions have no
/// argument expression and are already in the parameter's lane, so
/// they convert nothing; walking into them is what read `args` out of
/// range.
pub(crate) fn coerce_args_by_param_tys(
    ctx: &mut LowerCtx<'_>,
    param_tys: &[Type],
    args: &[ExprId],
    argv: &mut [Operand],
) -> Vec<(Operand, Type)> {
    let mut coerce_owned: Vec<(Operand, Type)> = Vec::new();
    for (i, expected) in param_tys.iter().enumerate() {
        if i >= argv.len() || i >= args.len() {
            break;
        }
        // RFC 20260707 chunk 626 — typed array into an Arr<Any>
        // param (T-11 call-boundary widen) marks the block's elem
        // kind for the callee's kind-aware readers.
        ctx.mark_arr_arg_for_any_param(*expected, &argv[i]);
        argv[i] = crate::ssa_lower_call_arg_conv::emit_arg_conv(
            ctx,
            *expected,
            args[i],
            argv[i],
            &mut coerce_owned,
        );
    }
    coerce_owned
}

fn coerce_to_f64_or_any_to_number(ctx: &mut LowerCtx<'_>, op: Operand) -> Operand {
    // S342 — Any-typed arg goes through any_to_number (ToNumber per
    // spec §21.3.2.*) so the F64 ABI sees a clean f64.
    // coerce_to_f64 wouldn't handle Any.
    if ctx.operand_ty(&op) == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(op)
    }
}
