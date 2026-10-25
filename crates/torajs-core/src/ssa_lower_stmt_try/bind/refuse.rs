//! What a typed catch binding does with a value its type does not
//! describe: a written type raises the TypeError, an inferred one (in
//! a checked build) stops the program. Moved out of the parent whole.

use crate::ssa::{InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

use super::ir::append;

/// Raise the TypeError for a rejected value through the catch's own
/// throw route, the value released.
pub(super) fn reject(ctx: &mut LowerCtx, p: &str, ann: &str, (tag, v): (ValueId, ValueId)) {
    let any_box = ctx.intrinsics.any_box;
    let pair = vec![Operand::Value(tag), Operand::Value(v)];
    let boxed = append(ctx, InstKind::Call(any_box, pair), Type::Any);
    ctx.emit_drop_value(Operand::Value(boxed), Type::Any);
    raise(ctx, p, ann);
}

/// The inferred type missed a raise: report it as uncaught and stop.
pub(super) fn missed(ctx: &mut LowerCtx, p: &str, ann: &str) {
    let msg = format!(
        "internal error: catch parameter `{p}` was inferred as `{ann}`, the thrown value is not one"
    );
    let msg = Operand::Value(ctx.intern_string_literal(msg.as_str()));
    let raiser = ctx.intrinsics.throw_type_error_str;
    let blk = ctx.cur_block;
    ctx.f.append_void(blk, InstKind::Call(raiser, vec![msg]));
    let exit = *(ctx.fn_table)
        .get(crate::ssa_lower_main_exit::UNCAUGHT_EXIT_SYM)
        .expect("__torajs_uncaught_exit declared in module setup");
    ctx.f.append_void(blk, InstKind::Call(exit, vec![]));
    ctx.f.set_term(blk, Terminator::Unreachable);
}

/// Record the TypeError of a rejected value and leave through the throw
/// route; the cursor moves to a fresh block no edge reaches.
pub(super) fn raise(ctx: &mut LowerCtx, p: &str, ann: &str) {
    let msg = format!("catch parameter `{p}` of type `{ann}` cannot hold the thrown value");
    let msg = Operand::Value(ctx.intern_string_literal(msg.as_str()));
    let raiser = ctx.intrinsics.throw_type_error_str;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(raiser, vec![msg]));
    let blk = ctx.cur_block;
    ctx.emit_throw_path(blk, None);
    ctx.cur_block = ctx.f.add_block();
}
