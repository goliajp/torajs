//! The throw-path release of one parked temp (`TempScratch::throw_live`)
//! — what `LowerCtx::emit_throw_check` runs for every live slot before
//! it branches to the catch or propagates. Split out of
//! `ssa_lower_emit_throw_check` with the `Close` shape; the park /
//! unpark bookkeeping stays there.

use crate::ssa::{FuncId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_temp_scratch::ThrowTemp;

impl<'a> LowerCtx<'a> {
    /// Park a stack record whose close kernel must run on a throw
    /// (see `ThrowTemp::Close`). Returns the token for
    /// [`Self::pop_throw_temp`].
    pub(crate) fn push_throw_close(&mut self, close: FuncId, record: ValueId) -> usize {
        self.temps
            .throw_live
            .push(Some(ThrowTemp::Close(close, record)));
        self.temps.throw_live.len() - 1
    }

    /// Throw-path release of one parked temp — the value's typed drop,
    /// or the slot's live pointee through the tag-dispatched heap drop.
    pub(crate) fn emit_drop_throw_temp(&mut self, t: ThrowTemp) {
        match t {
            ThrowTemp::Value(op, ty) => self.emit_drop_value(op, ty),
            ThrowTemp::Held(op, ty, hold) => {
                crate::ssa_lower::call_hold::emit_drop(self, hold, op, ty, false)
            }
            ThrowTemp::Slot(slot, ty) => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(ty, Operand::Value(slot), 0),
                    ty,
                    None,
                );
                self.emit_drop_value(Operand::Value(v), ty);
            }
            ThrowTemp::ArrSlotDeferLen(slot, ty, len_slot) => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(ty, Operand::Value(slot), 0),
                    ty,
                    None,
                );
                let len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(len_slot), 0),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(len), Operand::Value(v), ARR_LEN_OFF),
                );
                self.emit_drop_value(Operand::Value(v), ty);
            }
            ThrowTemp::DynobjSlot(slot) => {
                let p = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
                    Type::Ptr,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.value_drop_heap, vec![Operand::Value(p)]),
                );
            }
            ThrowTemp::Close(close, record) => {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(close, vec![Operand::Value(record)]),
                );
            }
        }
    }
}
