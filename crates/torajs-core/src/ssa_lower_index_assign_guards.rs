//! Per-store header-bit branches on the typed element-assign path —
//! the §10.4.2.1 integrity refusal (before the slot is written) and
//! the hole revive (after). Split out of `ssa_lower_index_assign.rs`
//! under the 500-line file rule when the integrity gate landed.
//!
//! Both read the SAME header word, and both are shaped so a plain
//! array pays one predictable not-taken branch: the flags that can
//! make an index special all live in the top u16 of the header, so
//! one mask decides whether the slow block runs at all.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// §10.4.2.1 — refuse a store into a non-writable index (an
    /// explicit `writable: false` shadow, or any index of a frozen
    /// array) before the slot is written. The `any`-receiver lane
    /// gates inside its kernel; the typed tier stores straight into
    /// the slot, so it needs the gate emitted.
    ///
    /// A plain array pays one predictable not-taken branch: the four
    /// header bits that can make an index non-writable
    /// (defineProperty shadow / frozen / sealed / non-extensible) test
    /// as ONE mask over the same header word the hole-revive branch
    /// below already loads, and only a hit calls the kernel.
    pub(crate) fn emit_index_integrity_guard(&mut self, arr_val: &Operand, idx_val: &Operand) {
        // Header word is `[refcount u32 | tag u16 | flags u16]`, so a
        // flags bit N sits at bit 48+N of the I64 load.
        const FLAGS_SHIFT: u32 = 48;
        let mask = ((torajs_rc::FLAG_ARR_EXOTIC_INDEX
            | torajs_rc::FLAG_FROZEN
            | torajs_rc::FLAG_SEALED
            | torajs_rc::FLAG_NON_EXTENSIBLE) as i64)
            << FLAGS_SHIFT;
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr_val.clone(), 0),
            Type::I64,
            None,
        );
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                crate::ssa::BinOp::And,
                Operand::Value(hdr),
                Operand::ConstI64(mask),
            ),
            Type::I64,
            None,
        );
        let guarded = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let check_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(guarded),
                then_blk: check_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = check_blk;
        self.f.append_void(
            check_blk,
            InstKind::Call(
                self.intrinsics.arr_index_check_store,
                vec![arr_val.clone(), idx_val.clone()],
            ),
        );
        // The kernel ARMS the TypeError (the `obj_check_not_frozen`
        // shape); this diverts to the user's handler before the slot
        // is written.
        self.emit_throw_check(None);
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(cont_blk));
        self.cur_block = cont_blk;
    }

    pub(crate) fn emit_hole_revive_branch(
        &mut self,
        arr_val: &Operand,
        idx_val: &Operand,
        join_blk: BlockId,
    ) {
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr_val.clone(), 0),
            Type::I64,
            None,
        );
        let exotic_bit = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                crate::ssa::BinOp::And,
                Operand::Value(hdr),
                Operand::ConstI64(i64::MIN),
            ),
            Type::I64,
            None,
        );
        let is_exotic = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(exotic_bit), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let revive_blk = self.f.add_block();
        let wb = self.cur_block;
        self.f.set_term(
            wb,
            Terminator::CondBr {
                cond: Operand::Value(is_exotic),
                then_blk: revive_blk,
                else_blk: join_blk,
            },
        );
        self.cur_block = revive_blk;
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_index_revive_idx,
                vec![arr_val.clone(), idx_val.clone()],
            ),
        );
        let rb = self.cur_block;
        self.f.set_term(rb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
    }
}
