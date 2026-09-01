//! `<Str>.concat | <Arr>.concat` dispatch — seventh sub-split
//! carved out of [`ssa_lower_str::try_lower_method_call`]. The Str
//! lane (a variadic left-fold over the 2-operand `str_concat`) lives
//! here; the three `<Arr>.concat` lanes moved to
//! [`ssa_lower_arr_concat`] (rotation 552, file-size move).
//!
//! Returns `None` for non-Str / non-Arr receivers or for the
//! `method != "concat"` case so the caller can keep trying the
//! remaining branches.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str>.concat(...)` or `<Arr>.concat(...)` through
/// the variadic left-fold dispatch. Returns `Some(value)` when
/// handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `s.concat(...others)` — variadic string concat,
    // lowered as a left-fold over str_concat. Empty arg
    // list returns the receiver unchanged. The single-arg
    // case still flows through the typecheck Function-arm
    // dispatch but we intercept here uniformly to avoid
    // duplicate emit paths.
    if recv_ty == Type::Str && method == "concat" {
        if args.is_empty() {
            // RFC 20260705 owned-result invariant: even the identity
            // shape answers an owned ref.
            ctx.emit_rc_inc(recv_op.clone());
            return Some(recv_op);
        }
        let mut acc = recv_op;
        // RFC 20260705 chunk 546 — the left-fold mints a fresh Str
        // per round; every non-final acc is a temp the next concat
        // only borrows. Drop it post-concat (round 0's acc is the
        // caller-owned receiver — untouched).
        let mut acc_fresh = false;
        for &a in args {
            // S212 — explicit `undefined` arg per ES §22.1.3.4
            // step 3.a: each arg is ToString'd, undefined →
            // "undefined". Inline-substitute the interned
            // literal so the helper sees a valid Str pointer
            // — same idiom S207/S211 use for replace/locale-
            // Compare.
            let mut other_fresh = false;
            let other = if matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined)) {
                let u = ctx.intern_string_literal("undefined");
                Operand::Value(u)
            } else {
                let v = ctx.lower_expr(a);
                // §22.1.3.5 step 3.b — ToString each argument. An
                // Any actual (the checker's any→Str admit: a String
                // wrapper object, a boxed primitive) carries a
                // NaN-box the raw str_concat kernel would deref as a
                // Str pointer (SIGSEGV on `'a'.concat(Object('b'))`)
                // — route it through the ToString kernel, which
                // answers an owned Str (html-wrap lane idiom).
                if ctx.operand_ty(&v) == Type::Any {
                    let s = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_to_str_box, vec![v]),
                        Type::Str,
                        None,
                    );
                    ctx.emit_throw_check(None);
                    other_fresh = true;
                    Operand::Value(s)
                } else {
                    v
                }
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_concat, vec![acc.clone(), other.clone()]),
                Type::Str,
                None,
            );
            if acc_fresh {
                ctx.emit_drop_value(acc, Type::Str);
            }
            if other_fresh {
                ctx.emit_drop_value(other, Type::Str);
            }
            acc = Operand::Value(v);
            acc_fresh = true;
        }
        return Some(acc);
    }
    // `arr.concat(other)` — fresh array, single malloc +
    // two memcpys via the C runtime. Element type carried.
    // Phase B refcount: derived array's slots alias both
    // sources; inc each slot for non-Copy elements.
    //
    // V3-18 wedge — multi-arg form `xs.concat(a, b, ..., z)`
    // per JS spec §22.1.3.2 is supported by folding the
    // single-arg intrinsic left-to-right: each step's
    // result becomes the next step's receiver. Refcount
    // inc runs once at the end over the final array's
    // full length. Each intermediate also leaks otherwise;
    // those temporaries are drop-balanced by the rc-inc
    // window on the final result (intermediates aren't
    // bound to a name so the surrounding scope-end drop
    // doesn't see them).
    if let Type::Arr(arr_id) = recv_ty
        && method == "concat"
    {
        return Some(crate::ssa_lower_arr_concat::lower_arr_concat(
            ctx, call_eid, recv_op, arr_id, args,
        ));
    }
    None
}
