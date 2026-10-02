//! Argument coercion for a direct call — how one lowered operand
//! becomes the type its parameter slot declares.
//!
//! Split out of `ssa_lower_call_terminal` in rotation 616, which had
//! grown to 498 lines. That file answers how a call is issued (which
//! target, which hidden slots, what the argument list looks like);
//! this one answers what each argument has to become on the way in —
//! the Math fast paths, the runtime default-literal bind, the
//! sig-aligned conversion walk, and the F64 sentinel cleanup.

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Clean the F64 `undefined` sentinel out of a Math argument. The
/// arg ExprId is absent only for synthesized calls, which never
/// carry a sentinel.
fn canon_math_arg(ctx: &mut LowerCtx<'_>, v: Operand, arg: Option<ExprId>) -> Operand {
    if ctx.operand_ty(&v) == Type::F64
        && arg.is_some_and(|e| crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, e))
    {
        return ctx.canon_f64_away_from_sentinel(v);
    }
    v
}

pub(crate) fn coerce_args(
    ctx: &mut LowerCtx<'_>,
    target: FuncId,
    headless: crate::ssa_lower_call_terminal_headless::HeadlessSlots,
    args: &[ExprId],
    argv: &mut [Operand],
) -> Vec<(Operand, Type)> {
    let hidden_off = headless.width();
    // ToNumber(undefined) is NaN — the F64 `undefined` sentinel must
    // not reach a Math kernel, which would hand its payload back out
    // (`Math.abs` clears the sign bit and returns the pattern
    // unchanged). See [`crate::ssa_lower_f64_sentinel_canon`].
    // The Math kernels take F64, and §21.3.2.* step 1 feeds them
    // ToNumber -- which reaches every value. A local helper used to
    // stand here special-casing `Any` and leaving everything else on
    // `coerce_to_f64`, which PANICS on an object: `Math.abs(o)` died at
    // lower time while `Math.abs(o as any)` answered. It also skipped
    // the throw check, so a `valueOf` that throws stranded its pending
    // throw behind a NaN placeholder. Both are `coerce_to_f64_tonumber`'s
    // job, and it is the only sink either arm needs.
    if ctx.is_math_unary(target) {
        debug_assert_eq!(argv.len(), 1, "Math.* unary takes 1 arg");
        argv[0] = ctx.coerce_to_f64_tonumber(argv[0]);
        argv[0] = canon_math_arg(ctx, argv[0], args.first().copied());
        return Vec::new();
    }
    if ctx.is_math_binary(target) {
        debug_assert_eq!(argv.len(), 2, "Math.* binary takes 2 args");
        for (i, a) in argv[..2].iter_mut().enumerate() {
            *a = ctx.coerce_to_f64_tonumber(*a);
            *a = canon_math_arg(ctx, *a, args.get(i).copied());
        }
        return Vec::new();
    }
    // §21.3.2.19 / §21.3.2.11 step 1 is ToUint32, not the ToNumber
    // the two arms above feed: these kernels take i64 and read its
    // low 32 bits. Same shape, different sink -- and going through
    // the generic sig walk below meant `FpToSi`, which is ToInteger:
    // `Math.clz32(Infinity)` answered 0 instead of 32.
    if let Some(n) = ctx.math_int32_arity(target) {
        debug_assert_eq!(argv.len(), n, "Math.* int32 arity");
        for a in argv[..n].iter_mut() {
            *a = ctx.coerce_to_i64_toint32(*a);
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
    // Cut the hidden pair out wherever it sits, so every zip below
    // stays user-aligned: at the front for a head-less callee, and
    // after the head for a ctor (rotation 616).
    let full = &ctx.fn_sigs[sig_id.0 as usize].0;
    let param_tys: Vec<Type> = if headless.at == 0 {
        full[hidden_off.min(full.len())..].to_vec()
    } else {
        let cut = headless.at.min(full.len());
        let mut v = full[..cut].to_vec();
        v.extend_from_slice(&full[(cut + hidden_off).min(full.len())..]);
        v
    };
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
