//! The §23.1.3 HasProperty gate the array callback methods run before
//! visiting an index.
//!
//! `forEach` / `map` / `filter` / `some` / `every` / `reduce` /
//! `reduceRight` all read "Let kPresent be ? HasProperty(O, Pk); If
//! kPresent is true, then …" — a hole is not visited, and the callback
//! never sees it. tr's loops walked `0..length` and called the callback
//! on every index, which is not merely an extra call: `[1, <hole>, 3]`
//! answered `some(v => v === undefined)` true where bun says false, and
//! `filter(() => true)` answered a three-element array where bun says
//! two.
//!
//! `find` / `findLast` / `findIndex` / `findLastIndex` are deliberately
//! NOT in that list — §23.1.3.9 Get's every index, holes included, and
//! tr already agrees with bun on them.
//!
//! **The hot path stays call-free.** Whether an index can be absent at
//! all is a property of the array's header: only an array carrying a
//! hole shadow (`FLAG_ARR_EXOTIC_INDEX`) or an unmaterialized tail
//! (`FLAG_ARR_SPARSE_TAIL`) has anything to say. One masked test on
//! that word — the same shape and the same word the typed element store
//! already pays for its integrity gate — decides whether the probe runs,
//! and the load is loop-invariant. A plain dense array pays one
//! predictable not-taken branch per element and never calls anything.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// Branch to `skip_blk` when index `i` is not an own property of
    /// `arr`. Leaves `cur_block` on the block where the element is
    /// present, which is where the caller emits the visit.
    pub(crate) fn emit_hof_present_gate(&mut self, arr: ValueId, i: ValueId, skip_blk: BlockId) {
        // Header word is `[refcount u32 | tag u16 | flags u16]`, so a
        // flags bit N sits at bit 48+N of the I64 load.
        const FLAGS_SHIFT: u32 = 48;
        let mask = ((torajs_rc::FLAG_ARR_EXOTIC_INDEX | torajs_rc::FLAG_ARR_SPARSE_TAIL) as i64)
            << FLAGS_SHIFT;
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(arr), 0),
            Type::I64,
            None,
        );
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(BinOp::And, Operand::Value(hdr), Operand::ConstI64(mask)),
            Type::I64,
            None,
        );
        let sparse = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let probe_blk = self.f.add_block();
        let work_blk = self.f.add_block();
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(sparse),
                then_blk: probe_blk,
                else_blk: work_blk,
            },
        );
        // Only an array that could hold a hole asks. The kernel is the
        // one `in` uses, so the two operators cannot disagree about
        // what is present.
        self.cur_block = probe_blk;
        let present = self.f.append_inst(
            probe_blk,
            InstKind::Call(
                self.intrinsics.arr_has_index,
                vec![Operand::Value(arr), Operand::Value(i)],
            ),
            Type::I64,
            None,
        );
        let ok = self.f.append_inst(
            probe_blk,
            InstKind::ICmp(IPred::Ne, Operand::Value(present), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            probe_blk,
            Terminator::CondBr {
                cond: Operand::Value(ok),
                then_blk: work_blk,
                else_blk: skip_blk,
            },
        );
        self.cur_block = work_blk;
    }
}
