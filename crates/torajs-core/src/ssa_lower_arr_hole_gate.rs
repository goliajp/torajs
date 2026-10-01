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
//! So the per-step gate is emitted into a loop a dense TYPED array
//! runs only where the callback may make an index absent mid-walk
//! (`hof_len_shrink`). A boxed-element source pays it unconditionally:
//! its loop body already carries a per-element `arr_get_any_boxed` call
//! that a branch disappears next to.
//!
//! A TYPED source can hold holes too — a write past the end or a
//! longer `length` leaves the gap absent in the cell's hole shadow
//! (`FLAG_ARR_EXOTIC_INDEX`) or as a sparse tail. Those arrays are rare
//! and the header says which ones they are, so the typed loop is
//! emitted twice ([`crate::ssa_lower_call_arr_ho`]): the preheader reads
//! the header once and a dense array runs today's loop unchanged; only
//! an array that can have an absent index runs the copy whose every
//! step asks `arr_has_index` ([`TypedGate::Present`]).
//!
//! Recorded boundary: a typed hole that a prototype digit key fills is
//! present, and the typed copy reads the slot word instead of that key.

use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// Which test a TYPED source's loop runs before visiting an index.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(crate) enum TypedGate {
    /// Nothing can make an index below the original length absent.
    None,
    /// The callback may shorten the array (`hof_len_shrink`).
    Len,
    /// The array can have holes: the loop copy the preheader picks for
    /// it asks `arr_has_index` at every step (which also covers `Len`).
    Present,
}

impl LowerCtx<'_> {
    /// Branch to `skip_blk` unless index `i` of the typed source `arr`
    /// is present now; `cur_block` is left where the visit is emitted.
    pub(crate) fn emit_hof_typed_gate(
        &mut self,
        gate: TypedGate,
        arr: ValueId,
        i: ValueId,
        skip_blk: BlockId,
    ) {
        match gate {
            TypedGate::None => {}
            TypedGate::Len => self.emit_hof_len_gate(arr, i, skip_blk),
            TypedGate::Present => {
                let visit_blk = self.f.add_block();
                let present = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_has_index,
                        vec![Operand::Value(arr), Operand::Value(i)],
                    ),
                    Type::I64,
                    None,
                );
                let ok = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Ne, Operand::Value(present), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                );
                let cur = self.cur_block;
                self.f.set_term(
                    cur,
                    Terminator::CondBr {
                        cond: Operand::Value(ok),
                        then_blk: visit_blk,
                        else_blk: skip_blk,
                    },
                );
                self.cur_block = visit_blk;
            }
        }
    }

    /// The whole §23.1.3 HasProperty test on a TYPED source whose
    /// callback may shorten it (`hof_len_shrink`): nothing else can
    /// make an index below the original length absent there. Re-read
    /// `length` and branch to `skip_blk` past it; `cur_block` is left
    /// where the visit is emitted. The loop bound stays the length
    /// read once before the loop (§23.1.3 step 3) — this is the
    /// any-source gate's dense arm, without the slot box.
    pub(crate) fn emit_hof_len_gate(&mut self, arr: ValueId, i: ValueId, skip_blk: BlockId) {
        let read_blk = self.f.add_block();
        let len_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(
                Type::I64,
                Operand::Value(arr),
                crate::ssa_lower::ARR_LEN_OFF,
            ),
            Type::I64,
            None,
        );
        let in_range = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i), Operand::Value(len_now)),
            Type::Bool,
            None,
        );
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(in_range),
                then_blk: read_blk,
                else_blk: skip_blk,
            },
        );
        self.cur_block = read_blk;
    }

    /// Read the array's header ONCE, in the loop's preheader: can any
    /// index of it be absent? Emitted before the loop so the body's
    /// branch is a register test on a loop-invariant value.
    pub(crate) fn emit_hof_sparse_probe(&mut self, arr: ValueId) -> ValueId {
        self.emit_arr_index_mop_probe(arr, torajs_rc::FLAG_ARR_SPARSE_TAIL)
    }

    /// An element read straight out of an `Arr<Any>` slot, with a slot
    /// hole (`FLAG_ARR_ANY_HOLEY` — the reserved encoding `0`) turned
    /// into the `undefined` a read of it answers. For the lanes that
    /// load raw slots and hand the value on (to a callback, into a
    /// collection) instead of going through the `arr_get_any_*`
    /// kernels, which do this themselves. (`Select` only forms after
    /// the egraph pass, so the choice is a slot and a branch.)
    pub(crate) fn emit_any_slot_hole_as_undefined(&mut self, v: ValueId) -> ValueId {
        let slot = self.alloca(Type::Any, Some("__hole_norm"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(v)),
            Type::I64,
            None,
        );
        let is_hole = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let undef_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(is_hole),
                then_blk: undef_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = undef_blk;
        let undef = self.f.append_inst(
            undef_blk,
            InstKind::IntToPtr(Operand::ConstI64(0x0A)),
            Type::Any,
            None,
        );
        self.f.append_void(
            undef_blk,
            InstKind::Store(Operand::Value(undef), Operand::Value(slot), 0),
        );
        self.f.set_term(undef_blk, Terminator::Br(cont_blk));
        self.cur_block = cont_blk;
        self.f.append_inst(
            cont_blk,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        )
    }

    /// Can an index of `arr` be anything but a plain own data property:
    /// a shadow hole / accessor (`FLAG_ARR_EXOTIC_INDEX`), a slot hole
    /// (`FLAG_ARR_ANY_HOLEY` — which means that only together with
    /// `FLAG_ARR_ANY`, bit 10 being the element kind on a typed cell,
    /// and a static `Arr<Any>` view can front a typed cell), or any of
    /// `extra`? The lowering's spelling of torajs-arr
    /// `layout::needs_index_mop`.
    pub(crate) fn emit_arr_index_mop_probe(&mut self, arr: ValueId, extra: u16) -> ValueId {
        // Header word is `[refcount u32 | tag u16 | flags u16]`, so a
        // flags bit N sits at bit 48+N of the I64 load.
        const FLAGS_SHIFT: u32 = 48;
        let any_mask = ((torajs_rc::FLAG_ARR_EXOTIC_INDEX | extra) as i64) << FLAGS_SHIFT;
        let holey =
            ((torajs_rc::FLAG_ARR_ANY | torajs_rc::FLAG_ARR_ANY_HOLEY) as i64) << FLAGS_SHIFT;
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(arr), 0),
            Type::I64,
            None,
        );
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(BinOp::And, Operand::Value(hdr), Operand::ConstI64(any_mask)),
            Type::I64,
            None,
        );
        let exotic = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let hbits = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(BinOp::And, Operand::Value(hdr), Operand::ConstI64(holey)),
            Type::I64,
            None,
        );
        let is_holey = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(hbits), Operand::ConstI64(holey)),
            Type::Bool,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(BinOp::Or, Operand::Value(exotic), Operand::Value(is_holey)),
            Type::Bool,
            None,
        )
    }

    /// The per-step HasProperty gate of a boxed-element source
    /// (rotation 652 — the earlier form asked the preheader's answer
    /// once and read every element as a borrowed slot box; a callback
    /// that shrank `length`, deleted a later index or defined an
    /// accessor on one was visited as if nothing had happened, and a
    /// getter-defined element never ran its getter: test262
    /// Array/prototype/{forEach,reduce,reduceRight} 7-b / 9-b / 9-c-i).
    ///
    /// Each step re-reads the header word (the loop already carries a
    /// call per element on this source, which is what the module doc's
    /// measurement exempts). Dense: `i < length` now, then the borrowed
    /// slot box. Exotic or sparse-tailed: the `in` kernel's answer, then
    /// the accessor-aware read (`arr_index_get`, +1-owned for cells),
    /// remembered in `owned_slot` / `owned_flag` for the step block to
    /// release. Either way the element lands in `elem_slot`, and
    /// `cur_block` is left where the visit is emitted; an absent index
    /// goes to `skip_dense` from the dense arm (the length shrank) and
    /// to `skip_exotic` from the other (a hole, or an index past a
    /// sparse tail) — the two are told apart because only the second
    /// is announced by a header bit with a writer entry.
    pub(crate) fn emit_hof_present_gate(
        &mut self,
        arr: ValueId,
        i: ValueId,
        skip_dense: BlockId,
        skip_exotic: BlockId,
        elem_slot: ValueId,
        owned_slot: ValueId,
        owned_flag: ValueId,
    ) {
        let probe_blk = self.f.add_block();
        let dense_blk = self.f.add_block();
        let dense_read_blk = self.f.add_block();
        let exotic_read_blk = self.f.add_block();
        let work_blk = self.f.add_block();
        // Cleared before the branch: an ABSENT index jumps to the step
        // block without passing either read arm, and the step block
        // must not find the previous step's flag still standing (it
        // released that box already — a second release was the
        // crash the first version of this gate shipped to the slice).
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(owned_flag), 0),
        );
        let sparse_now = self.emit_hof_sparse_probe(arr);
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(sparse_now),
                then_blk: probe_blk,
                else_blk: dense_blk,
            },
        );
        // Dense: every index below the CURRENT length is present.
        self.cur_block = dense_blk;
        let len_now = self.f.append_inst(
            dense_blk,
            InstKind::Load(
                Type::I64,
                Operand::Value(arr),
                crate::ssa_lower::ARR_LEN_OFF,
            ),
            Type::I64,
            None,
        );
        let in_range = self.f.append_inst(
            dense_blk,
            InstKind::ICmp(IPred::Slt, Operand::Value(i), Operand::Value(len_now)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            dense_blk,
            Terminator::CondBr {
                cond: Operand::Value(in_range),
                then_blk: dense_read_blk,
                else_blk: skip_dense,
            },
        );
        self.cur_block = dense_read_blk;
        let borrowed = crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            self,
            Operand::Value(arr),
            Operand::Value(i),
        );
        let read_end = self.cur_block;
        self.f.append_void(
            read_end,
            InstKind::Store(Operand::Value(borrowed), Operand::Value(elem_slot), 0),
        );
        self.f.append_void(
            read_end,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(owned_flag), 0),
        );
        self.f.set_term(read_end, Terminator::Br(work_blk));
        // Exotic: the kernel `in` uses, so the two operators cannot
        // disagree about what is present; then the read that runs a
        // getter.
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
                then_blk: exotic_read_blk,
                else_blk: skip_exotic,
            },
        );
        self.cur_block = exotic_read_blk;
        let owned = self.f.append_inst(
            exotic_read_blk,
            InstKind::Call(
                self.intrinsics.arr_index_get,
                vec![Operand::Value(arr), Operand::Value(i)],
            ),
            Type::Any,
            None,
        );
        // The getter is user code.
        self.emit_throw_check(None);
        let rb = self.cur_block;
        self.f.append_void(
            rb,
            InstKind::Store(Operand::Value(owned), Operand::Value(elem_slot), 0),
        );
        self.f.append_void(
            rb,
            InstKind::Store(Operand::Value(owned), Operand::Value(owned_slot), 0),
        );
        self.f.append_void(
            rb,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(owned_flag), 0),
        );
        self.f.set_term(rb, Terminator::Br(work_blk));
        self.cur_block = work_blk;
    }

    /// Step-block half of [`Self::emit_hof_present_gate`]: release the
    /// exotic arm's owned element, if this step made one. A branch
    /// around the call, so the dense path pays a flag load and nothing
    /// else.
    pub(crate) fn emit_hof_release_owned(&mut self, owned_slot: ValueId, owned_flag: ValueId) {
        let drop_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        let flag = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(owned_flag), 0),
            Type::Bool,
            None,
        );
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(flag),
                then_blk: drop_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = drop_blk;
        let v = self.f.append_inst(
            drop_blk,
            InstKind::Load(Type::Any, Operand::Value(owned_slot), 0),
            Type::Any,
            None,
        );
        self.emit_drop_value(Operand::Value(v), Type::Any);
        let db = self.cur_block;
        self.f.set_term(db, Terminator::Br(cont_blk));
        self.cur_block = cont_blk;
    }
}
