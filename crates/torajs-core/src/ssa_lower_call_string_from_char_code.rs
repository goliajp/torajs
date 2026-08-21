//! `String.fromCharCode(...codes)` / `String.fromCodePoint(...codes)`
//! variadic-arg formatter pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s `Expr::Call` god-arm as
//! chunk-45 of the decomp (chunks 1-44 = ... + Array.isArray).
//!
//! Each code is converted to a one-char string via the appropriate
//! runtime helper, then pairwise `__torajs_str_concat` builds the
//! final string. Single-arg is the hot path; multi-arg builds an
//! O(n) chain. Empty arg list yields `""`.
//!
//! **P11.1-S6 split**:
//! - `fromCharCode` → `__torajs_str_from_char_code`. Truncates
//!   `n & 0xFFFF`, never throws.
//! - `fromCodePoint` → `__torajs_str_from_code_point` per ES
//!   §22.1.2.2. Accepts full codepoint `[0, 0x10FFFF]` (surrogate-
//!   pair encodes supplementary plane), raises catchable
//!   `RangeError` on out-of-range. The per-arg
//!   `emit_throw_check(None)` after each call propagates the throw
//!   before the concat consumes the (possibly invalid) result.
//!
//! **Spec carve-outs preserved**:
//! - **S231** — `String.fromCharCode(undefined)` per ES §22.1.2.1:
//!   `ToUint16(undefined) = 0`. The `check::S231` carve-out lets
//!   the typed-`Undefined` arg through the type gate; we substitute
//!   `ConstI64(0)` here so the `ConstPtrNull` undef sentinel never
//!   reaches the helper's i64 ABI.
//! - **S329** — `String.fromCharCode(Any)` per ES §22.1.2.1
//!   `ToUint16`: decode Any via `__torajs_anyv_to_number` →
//!   `coerce_to_i64`. The helper sig is `(i64) -> Str` so the f64
//!   intermediate gets FpToSi'd at the i64 boundary.
//! - **S340** — same path covers `fromCodePoint(Any)`; runtime
//!   helper `__torajs_str_from_code_point` still throws RangeError
//!   on non-finite / OOR, propagated by the per-arg
//!   `emit_throw_check` below for `is_from_code_point`.
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `String.fromCharCode` / `String.fromCodePoint` Member-Ident
//! shape).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Member {
        obj: ns_id,
        name: m_name,
    } = ctx.ast.get_expr(callee)
    else {
        return None;
    };
    let is_from_code_point = m_name == "fromCodePoint";
    if !is_from_code_point && m_name != "fromCharCode" {
        return None;
    }
    let ns_id = *ns_id;
    let Expr::Ident(ns) = ctx.ast.get_expr(ns_id) else {
        return None;
    };
    if ns != "String" {
        return None;
    }
    if args.is_empty() {
        return Some(Operand::Value(ctx.intern_string_literal("")));
    }
    // Both kernels take the Number the spec hands their step, not an
    // integer: §22.1.2.1's `ToUint16` maps ±∞ and NaN to +0, and
    // §22.1.2.2 step 2.b rejects a NON-INTEGRAL code point. An i64
    // parameter can express neither — whoever truncates on the way in
    // has already destroyed the fact each check is about, which is how
    // `fromCharCode(Infinity)` became U+FFFF and how
    // `fromCodePoint('3.14')` would have become U+0003 rather than a
    // RangeError.
    let intrinsic = if is_from_code_point {
        ctx.intrinsics.str_from_code_point_f64
    } else {
        ctx.intrinsics.str_from_char_code_f64
    };
    let mut acc: Option<Operand> = None;
    for &aid in args.iter() {
        // Every operand shape is accepted and coerced: a Number goes
        // straight to the kernel, everything else through the
        // runtime's own ToNumber. `undefined` needs no special case
        // any more — it boxes to ANY_UNDEF, ToNumber's it to NaN, and
        // each kernel then gives NaN the answer its own spec step
        // asks for (0 for fromCharCode, RangeError for fromCodePoint).
        let v = ctx.lower_to_number_operand(aid);
        let n = ctx.coerce_to_f64(v);
        let cur_block = ctx.cur_block;
        let one = ctx.f.append_inst(
            cur_block,
            InstKind::Call(intrinsic, vec![n]),
            Type::Str,
            None,
        );
        if is_from_code_point {
            ctx.emit_throw_check(None);
        }
        let one_op = Operand::Value(one);
        acc = Some(match acc {
            None => one_op,
            Some(prev) => {
                let cur_block = ctx.cur_block;
                let cat = ctx.f.append_inst(
                    cur_block,
                    InstKind::Call(
                        ctx.intrinsics.str_concat,
                        vec![prev.clone(), one_op.clone()],
                    ),
                    Type::Str,
                    None,
                );
                // str_concat borrows its inputs (chunk 664 ledger)
                // — both fresh rc=1 temps release here or every
                // multi-arg call leaks the whole intermediate chain.
                ctx.emit_drop_value(prev, Type::Str);
                ctx.emit_drop_value(one_op, Type::Str);
                Operand::Value(cat)
            }
        });
    }
    Some(acc.expect("variadic fromCharCode acc must have been set"))
}
