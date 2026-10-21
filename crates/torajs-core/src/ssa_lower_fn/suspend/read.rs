//! The read of an awaited cell's settled value word (`super`).

use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// `torajs-promise` `Promise`: the word at +8 holds `state`,
/// `value_is_heap`, `has_handler` and `value_repr`, one byte each from
/// the low end; `value` is at +16 (its layout test pins both).
pub(super) const PROMISE_STATE_WORD_OFF: u64 = 8;
pub(super) const PROMISE_VALUE_OFF: u64 = 16;
/// `state` and `value_repr` out of that word.
const STATE_AND_REPR_MASK: u32 = 0xFF00_00FF;
/// `torajs-promise` `STATE_FULFILLED`.
const STATE_FULFILLED: i32 = 1;

/// The state word of a cell fulfilled in the form `want_repr` names,
/// and the bytes of it that tell (`torajs-promise`
/// `module_await::state_word_wanted` builds the same two).
pub(super) fn state_word_wanted(want_repr: i64) -> (u32, u32) {
    if want_repr == 0 {
        (0xFF, STATE_FULFILLED as u32)
    } else {
        (
            STATE_AND_REPR_MASK,
            ((want_repr as u32) << 24) | STATE_FULFILLED as u32,
        )
    }
}

/// The settled cell's value word as `want_repr` asks for it; a
/// rejected source raises its reason here. A cell fulfilled in that
/// very form holds the word itself, read in place; any other goes
/// through `__torajs_promise_get_value_as`, which converts the form or
/// raises, and marks the cell handled (the mark matters only to a
/// rejected cell: the exit's rejection sweep is its one reader).
/// `settled` must not be NULL: the in-place path loads its state word
/// before anything else, unlike `__torajs_promise_get_value_as`, which
/// tolerates a NULL cell.
pub(super) fn read_settled(
    ctx: &mut LowerCtx<'_>,
    settled: Operand,
    want_repr: i64,
) -> crate::ssa::ValueId {
    if want_repr == 0 {
        return read_settled_call(ctx, settled, want_repr);
    }
    let slot = ctx.alloca_in_entry(Type::I64, Some("__awaitv"));
    let cb = ctx.cur_block;
    let word = InstKind::Load(Type::I32, settled, PROMISE_STATE_WORD_OFF);
    let word = ctx.f.append_inst(cb, word, Type::I32, None);
    let mask = Operand::ConstI32(STATE_AND_REPR_MASK as i32);
    let masked = InstKind::BinOp(BinOp::And, Operand::Value(word), mask);
    let masked = ctx.f.append_inst(cb, masked, Type::I32, None);
    let want = Operand::ConstI32(((want_repr as i32) << 24) | STATE_FULFILLED);
    let hit = InstKind::ICmp(IPred::Eq, Operand::Value(masked), want);
    let hit = ctx.f.append_inst(cb, hit, Type::Bool, None);
    let in_place = ctx.f.add_block();
    let call = ctx.f.add_block();
    let join = ctx.f.add_block();
    let cond = Operand::Value(hit);
    let term = Terminator::CondBr {
        cond,
        then_blk: in_place,
        else_blk: call,
    };
    ctx.f.set_term(cb, term);
    let v = InstKind::Load(Type::I64, settled, PROMISE_VALUE_OFF);
    let v = ctx.f.append_inst(in_place, v, Type::I64, None);
    let store = InstKind::Store(Operand::Value(v), Operand::Value(slot), 0);
    ctx.f.append_void(in_place, store);
    ctx.f.set_term(in_place, Terminator::Br(join));
    ctx.cur_block = call;
    let raw = read_settled_call(ctx, settled, want_repr);
    let end = ctx.cur_block;
    let store = InstKind::Store(Operand::Value(raw), Operand::Value(slot), 0);
    ctx.f.append_void(end, store);
    ctx.f.set_term(end, Terminator::Br(join));
    ctx.cur_block = join;
    let load = InstKind::Load(Type::I64, Operand::Value(slot), 0);
    ctx.f.append_inst(join, load, Type::I64, None)
}

/// [`read_settled`] through the runtime.
fn read_settled_call(
    ctx: &mut LowerCtx<'_>,
    settled: Operand,
    want_repr: i64,
) -> crate::ssa::ValueId {
    let cb = ctx.cur_block;
    let raw = ctx.f.append_inst(
        cb,
        InstKind::Call(
            ctx.intrinsics.promise_get_value_as,
            vec![settled, Operand::ConstI64(want_repr)],
        ),
        Type::I64,
        None,
    );
    ctx.emit_throw_check_owned(None, settled, Type::Promise);
    raw
}
