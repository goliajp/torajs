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
//! **The hot path stays call-free, and the test is not in the loop.**
//! Whether an index can be absent at all is a property of the array's
//! header: only an array carrying a hole shadow
//! (`FLAG_ARR_EXOTIC_INDEX`) or an unmaterialized tail
//! (`FLAG_ARR_SPARSE_TAIL`) has anything to say. That word is read ONCE,
//! in the loop's preheader, and the loop carries the answer as a plain
//! boolean.
//!
//! The first version read it inside the body — the same masked test the
//! typed element store pays for its integrity gate — reasoning that the
//! load was loop-invariant and a dense array would pay one predictable
//! not-taken branch. **Measured: 79.8 ms → 106.7 ms, +34%**, on a
//! 3M-element `forEach`/`reduce`/`some`/`every` probe (hyperfine -N,
//! 20 runs, σ ≤ 0.6 ms). Hoisting the load by hand — so the condition
//! is a genuinely loop-invariant SSA value — bought 34% → **29%**.
//! Two rounds, no movement: the branch itself is not the price. What
//! costs is that it splits the loop body into blocks, and a
//! `sum += x` loop that was one block no longer unrolls or vectorizes.
//!
//! So the gate is not emitted at all unless the source's ELEMENT TYPE
//! is `any`. That is not a compromise about which arrays get correct
//! answers — it is the same fact the delete side is built on: an
//! unboxed slot has no value that means absent, so `delete a[i]` on a
//! `number[]` widens the declaration to `any[]` rather than punching a
//! hole into one ([`crate::ast::delete_arr_widen`]). An interior hole
//! can only exist behind boxed elements, and there the loop body
//! already carries a per-element `arr_get_any_boxed` call that a
//! branch disappears next to.
//!
//! Recorded boundary: `a.length = 5` on a `number[]` leaves a sparse
//! TAIL — indices past the live extent that are absent without any
//! boxed slot. Those keep today's answer (the callback runs on them).
//! It is a suffix, not an interior hole, so the fix is a preheader
//! clamp of the loop bound rather than a per-element test — no hot-path
//! cost, and no reason to buy it with one.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// Read the array's header ONCE, in the loop's preheader: can any
    /// index of it be absent? Emitted before the loop so the body's
    /// branch is a register test on a loop-invariant value.
    pub(crate) fn emit_hof_sparse_probe(&mut self, arr: ValueId) -> ValueId {
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
        self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        )
    }

    /// Branch to `skip_blk` when index `i` is not an own property of
    /// `arr`. `sparse` is [`Self::emit_hof_sparse_probe`]'s answer from
    /// the preheader. Leaves `cur_block` on the block where the element
    /// is present, which is where the caller emits the visit.
    pub(crate) fn emit_hof_present_gate(
        &mut self,
        sparse: ValueId,
        arr: ValueId,
        i: ValueId,
        skip_blk: BlockId,
    ) {
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
