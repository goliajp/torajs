//! RFC 20260718-error-message-own-prop — the injected-error-ctor
//! magic-ident lowerings, split from `ssa_lower_call_class_synth.rs`
//! (its dispatch match routes here) to keep that file under the
//! 500-line HARD limit:
//!
//! - `__torajs_undef_str()` — 刀 2 own-absence Str sentinel mint.
//! - `__torajs_error_stack(this)` — 刀 2 §20.5.3.4 stack header via
//!   the `error_to_string` runtime helper.
//! - `__torajs_ctor_no_super_throw()` — 刀 3 §9.2.2 this-TDZ
//!   ReferenceError raiser at super-less derived ctor tails.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::LowerCtx;

/// RFC 20260718-error-message-own-prop 刀 2 —
/// `__torajs_undef_str()` in the injected error ctors: the
/// own-absence Str sentinel address (GlobalRef mint, zero calls).
/// A missing ctor `message` stores this into the field slot, which
/// every reflection surface reads as "no own message property".
pub(crate) fn try_lower_undef_str(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if !args.is_empty() {
        return None;
    }
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::GlobalRef(crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM.to_string()),
        crate::ssa::Type::Str,
        None,
    );
    Some(Operand::Value(v))
}

/// 刀 3 — `__torajs_ctor_no_super_throw()` at the tail of a
/// super-less derived user ctor (§9.2.2 [[Construct]] this-TDZ):
/// records the pending catchable ReferenceError; the emitted
/// throw-check propagates it through the factory to the `new` site.
pub(crate) fn try_lower_ctor_no_super_throw(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if !args.is_empty() {
        return None;
    }
    let cur_block = ctx.cur_block;
    let raiser = ctx.intrinsics.ctor_no_super_throw;
    ctx.f
        .append_void(cur_block, InstKind::Call(raiser, Vec::new()));
    ctx.emit_throw_check(None);
    Some(Operand::ConstI64(0))
}

/// 刀 2 — `__torajs_error_stack(this)` in the injected error ctors:
/// the §20.5.3.4 stack header (`name` / `name + ": " + message`,
/// undefined-or-empty message → bare name) via the
/// `__torajs_error_to_string` runtime helper, replacing the old
/// AST-level ternary concat that couldn't see the sentinel. Owned
/// Str result (the `this.stack = ...` store consumes it).
pub(crate) fn try_lower_error_stack(ctx: &mut LowerCtx<'_>, args: &[ExprId]) -> Option<Operand> {
    if args.len() != 1 {
        return None;
    }
    let this_op = ctx.lower_expr(args[0]);
    let cur_block = ctx.cur_block;
    let error_to_string = ctx.intrinsics.error_to_string;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(error_to_string, vec![this_op]),
        crate::ssa::Type::Str,
        None,
    );
    Some(Operand::Value(v))
}
