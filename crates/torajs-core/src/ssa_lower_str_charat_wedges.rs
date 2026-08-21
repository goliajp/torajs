//! `charAt` / `charCodeAt` / `codePointAt` spec-default wedges for
//! Str | Substr receivers — second sub-split carved out of
//! [`ssa_lower_str::try_lower_method_call`] along the
//! `ssa_lower_str/{view,search,transform,structural}.rs` axis.
//!
//! Encapsulates two pre-dispatch wedges that share the
//! `charAt`/`charCodeAt`/`codePointAt` spec defaults:
//! - 0-arg form (ES §22.1.3.2 / §22.1.3.3 / §22.1.3.4 step 2-3:
//!   missing `pos` defaults to 0) — synthesizes a `ConstI64(0)`
//!   index and routes through the matching `__torajs_str_*` /
//!   `__torajs_substr_*` intrinsic.
//! - 1+ arg `charAt(i, ...trailing)` fast path (S272 trailing-arg
//!   ignore + S222 undefined / S332 Any operand decode) that emits a
//!   length-1 view slice rather than a separate runtime helper.
//!
//! Returns `None` when the receiver is not `Type::Str | Type::Substr`
//! or the method/arg shape does not match either wedge, so the
//! caller can fall through to the generic Str-path dispatch.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Try to lower `<Str | Substr>.charAt(...)` /
/// `.charCodeAt(...)` / `.codePointAt(...)` through the
/// spec-default wedges. Returns `Some(value)` when one of the
/// wedges fired; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if !matches!(recv_ty, Type::Str | Type::Substr) {
        return None;
    }
    // V3-18 wedge — charAt / charCodeAt /
    // codePointAt 0-arg form per JS spec
    // §22.1.3.4 / §22.1.3.5 / §22.1.3.6: missing
    // pos defaults to 0. Synthesize a ConstI64(0)
    // index and route through the existing 1-arg
    // paths below.
    if matches!(method, "charAt" | "charCodeAt" | "codePointAt") && args.is_empty() {
        let idx_val = Operand::ConstI64(0);
        if method == "charAt" {
            let v = if recv_ty == Type::Str {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                    Type::Substr,
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.substr_slice,
                        vec![recv_op, idx_val, Operand::ConstI64(1)],
                    ),
                    Type::Substr,
                    None,
                )
            };
            return Some(Operand::Value(v));
        }
        // P11.3-A1 — split charCodeAt vs codePointAt (the latter
        // combines surrogate pairs per ES §22.1.3.3). 0-arg form
        // still applies: `'😀'.codePointAt()` should default pos
        // to 0 and return 0x1F600, not 0xD83D.
        let target = if method == "codePointAt" {
            if recv_ty == Type::Str {
                ctx.intrinsics.str_code_point_at
            } else {
                ctx.intrinsics.substr_code_point_at
            }
        } else if recv_ty == Type::Str {
            ctx.intrinsics.str_char_code_at
        } else {
            ctx.intrinsics.substr_char_code_at
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, vec![recv_op, idx_val]),
            Type::I64,
            None,
        );
        return Some(Operand::Value(v));
    }
    // `s.charAt(i)` — same-shape alias for `s[i]`.
    // Lowers to a length-1 substr view instead of going
    // through a separate runtime helper.
    // S240 widens the 1-arg detection to 2-arg so the
    // charAt(idx, trailing) shape still routes through this length-1
    // substr-view fast path; args[1] is never lowered (trailing-arg
    // ignore).
    //
    // S272 — widen `== 1 || == 2` to `>= 1` so charAt(idx, ...trailing)
    // with any trailing count routes through this fast path; the
    // trailing exprs are eval-and-dropped below so side effects fire.
    if method == "charAt" && !args.is_empty() {
        // S222 — `s.charAt(undefined)` per ES §22.1.3.2 step 2-3:
        // ToIntegerOrInfinity(undefined)=0. Short-circuit to ConstI64(0)
        // before coerce_to_i64, which can't lower a ConstPtrNull undef
        // sentinel.
        let arg0_undef = matches!(
            ctx.expr_types.get(&args[0]),
            Some(crate::check::Type::Undefined)
        );
        // S332 — `s.charAt(x)` per ES §22.1.3.2: ToIntegerOrInfinity
        // accepts arbitrary-typed input, so the operand is COERCED
        // rather than shape-checked. `lower_to_number_operand` keeps a
        // Number on the typed-tier fast path (no box, no call) and
        // routes every other shape — Str, Bool, Any, a cell with a
        // `valueOf` — through the runtime's own ToNumber; the
        // `coerce_to_i64` below then folds NaN/±∞ per ToInteger so the
        // helper's `(Str|Substr, i64)` ABI sees a clean index.
        let idx_val = if arg0_undef {
            Operand::ConstI64(0)
        } else {
            let n = ctx.lower_to_number_operand(args[0]);
            ctx.coerce_to_i64(n)
        };
        let v = if recv_ty == Type::Str {
            // V3-18 m1.h.37 — bounds-checked str charAt.
            // Pre-fix called substr_create directly; OOB
            // indices stored garbage offsets and printed
            // bytes from past the parent's data.
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_char_at, vec![recv_op, idx_val]),
                Type::Substr,
                None,
            )
        } else {
            let end = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Add, idx_val, Operand::ConstI64(1)),
                Type::I64,
                None,
            );
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.substr_slice,
                    vec![recv_op, idx_val, Operand::Value(end)],
                ),
                Type::Substr,
                None,
            )
        };
        // S272 — eval-and-drop trailing exprs so side effects fire
        // per ES §22.1.3.2 trailing-arg ignore semantics.
        for &a in &args[1..] {
            let _ = ctx.lower_expr(a);
        }
        return Some(Operand::Value(v));
    }
    None
}
