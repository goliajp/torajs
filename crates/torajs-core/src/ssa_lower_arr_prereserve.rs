//! The pre-reserved fast push — the three emit shapes that a
//! [`PreReserveState`](crate::ssa_lower::PreReserveState) is made of.
//!
//! A caller that has already proved the destination cannot grow
//! (`arr_reserve` above the loop, and nothing in the body can shift or
//! unshift it) owns three loop-invariant facts: the data base, the byte
//! offset of slot[0], and a running length living in an alloca that
//! mem2reg promotes to a register. Appending is then a slot store and an
//! add — no call, no capacity test, and no read of the cell's own length
//! word, which would put a store-to-load forward on the loop-carried
//! dependency chain.
//!
//! The price is that the length word is STALE for the duration of the
//! loop; [`LowerCtx::emit_prereserved_len_writeback`] settles it at the
//! single normal exit, so every reader outside the loop passes through
//! it. A throw out of the body skips that settlement — the shape is only
//! taken where the loop's own preheader owns the reservation, and the
//! `for` / `while` lanes accepted that boundary when they opened it.
//!
//! What the stale word means for a REFCOUNTED slot is worth stating,
//! because it is the question this shape invites and the answer is not
//! "don't do that". Every runtime walker bounds an array by its `len`
//! (`torajs_cycle::arr::arr_len_of`), so the slots written so far are
//! invisible to a collection that runs mid-loop. Invisibility can only
//! cost collection, never soundness: each slot's stake is counted the
//! moment it is stored, so a cell reachable only through one of them
//! carries an rc the trial deletion never finds a reason to decrement,
//! and is kept. The window where a dead cycle survives one collection
//! closes at the writeback.

use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, PreReserveState};

impl LowerCtx<'_> {
    /// Read the three loop-invariant facts off an array the caller has
    /// just reserved, and mint the running-length slot. Emit this in the
    /// preheader — `reserved` is the pointer `arr_reserve` answered, and
    /// B1 says the cell never moves, so all four stay valid for the whole
    /// loop.
    pub(crate) fn emit_prereserved_state(&mut self, reserved: ValueId) -> PreReserveState {
        let head_off = match self.emit_arr_head_x8(Operand::Value(reserved)) {
            Operand::Value(v) => v,
            _ => unreachable!("emit_arr_head_x8 returns a value"),
        };
        let data_ptr = match self.emit_arr_data_ptr(Operand::Value(reserved)) {
            Operand::Value(v) => v,
            _ => unreachable!("emit_arr_data_ptr returns a value"),
        };
        let len_after = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(reserved), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let len_slot = self.alloca(Type::I64, Some("__push_len"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(len_after), Operand::Value(len_slot), 0),
        );
        PreReserveState {
            arr_ptr: reserved,
            data_ptr,
            head_off,
            len_slot,
        }
    }

    /// Append `val` at the running length: store the slot, bump the
    /// count. Answers the NEW length, which is what JS `push` answers.
    ///
    /// `val` crosses as its own type. This is a direct slot store, not a
    /// `__torajs_arr_*` argument, so `raw_slot_arg`'s f64 → i64 bit
    /// crossing is neither needed nor wanted here.
    pub(crate) fn emit_prereserved_push(&mut self, st: PreReserveState, val: Operand) -> ValueId {
        let len_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(st.len_slot), 0),
            Type::I64,
            None,
        );
        let len_x8 = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Mul, Operand::Value(len_now), Operand::ConstI64(8)),
            Type::I64,
            None,
        );
        let byte_off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(st.head_off),
                Operand::Value(len_x8),
            ),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(val, Operand::Value(st.data_ptr), Operand::Value(byte_off)),
        );
        let len_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(len_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(len_next), Operand::Value(st.len_slot), 0),
        );
        len_next
    }

    /// Settle the cell's length word from the running count.
    pub(crate) fn emit_prereserved_len_writeback(&mut self, st: PreReserveState) {
        let final_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(st.len_slot), 0),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(final_len),
                Operand::Value(st.arr_ptr),
                ARR_LEN_OFF,
            ),
        );
    }
}
