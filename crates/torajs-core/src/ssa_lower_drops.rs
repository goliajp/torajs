//! Fn-exit drop emission — owned-local and module-global cleanup
//! sequences appended at every fall-through / return exit (extracted
//! from `ssa_lower.rs`, file-size known-debt #1).
//!
//! Both emitters walk HashMap-backed tables, so both sort before
//! emitting: HashMap iteration order is random per process, and an
//! unsorted walk leaks that randomness into the emitted drop-call
//! sequence, making `tr build` output non-reproducible (the bench
//! artifact-hash gate then flags spurious diffs on every tr rebuild).

use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::{LocalInfo, LowerCtx};

impl LowerCtx<'_> {
    /// RFC 20260710 — was this (shadow-restored) binding promoted to
    /// a non-Copy capture box by its let-decl site? Re-derives the
    /// promotion predicate — the scope-close set cleanup removes the
    /// inner name, and the restore re-inserts iff the OUTER binding
    /// was itself boxed (owner-marked: moved, not borrowed).
    pub(crate) fn binding_is_boxed_noncopy(&self, name: &str, info: &LocalInfo) -> bool {
        info.moved
            && !info.borrowed
            && !info.ty.is_copy()
            && info.ty != Type::Substr
            && self.escape_captured_lets.contains(name)
            && self.mutated_captured_lets.contains(name)
    }

    /// RFC 20260710 — the box-release intrinsic matching a promoted
    /// capture's payload type: plain Copy box / universal heap
    /// content / NaN-box Any content (C4).
    pub(crate) fn capture_box_drop_fid(&self, ty: Type) -> crate::ssa::FuncId {
        if ty.is_copy() {
            self.intrinsics.capture_box_drop
        } else if ty == Type::Any {
            self.intrinsics.capture_box_drop_any
        } else {
            self.intrinsics.capture_box_drop_heap
        }
    }

    pub(crate) fn emit_drops_for_owned_locals(&mut self) {
        // Snapshot to avoid borrowing self.locals while we emit instructions
        // (which need &mut self.f). Cheap: bench cases have <10 locals each.
        // 11-A2-a — skip stack-alloced locals: their backing storage is
        // reclaimed by fn return; no rc-dec / obj_drop_sized needed.
        // r503 — a class prologue cell (`__class_<C>` / `__proto_<C>`,
        // `: any`, a local of the fn that declared the class) releases
        // through a link seam the judgment NOPs together with the
        // register call: a cell the registry never held and no
        // reader can reach is unobservable after exit, and the
        // generic any-drop it would otherwise call roots the cycle /
        // weak worlds from the user main.
        let mut to_drop: Vec<(ValueId, Type, bool)> = self
            .locals
            .iter()
            .filter(|(name, info)| {
                !info.moved && !info.ty.is_copy() && !self.stack_alloced_locals.contains(*name)
            })
            .map(|(name, info)| {
                let class_cell = info.ty == Type::Any
                    && crate::ssa_lower_closure::class_sentinel_name(self, name);
                (info.slot, info.ty, class_cell)
            })
            .collect();
        // Drop in declaration reverse (slot ids are allocated in
        // declaration order), the textbook LIFO destruction order.
        to_drop.sort_by_key(|&(slot, _, _)| std::cmp::Reverse(slot.0));
        for (slot, ty, class_cell) in to_drop {
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(slot), 0),
                ty,
                None,
            );
            if class_cell {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.class_cell_release,
                        vec![Operand::Value(val)],
                    ),
                );
            } else {
                self.emit_drop_value(Operand::Value(val), ty);
            }
        }
        // RFC 20260705 chunk 550 fix-up — escape-promoted Copy locals
        // live in refcounted capture boxes (outer-stake protocol:
        // alloc = rc 1, each capturing env +1). Release the frame's
        // own stake so the box frees once the last capturing env
        // drops; without the env-temp release of chunk 550 the envs
        // never dropped and the box leaked instead.
        //
        // `!info.borrowed` mirrors the non-Copy arm below: a
        // closure-body preamble byref binding BORROWS its box (the
        // env owns that stake). A body that mints a nested closure
        // over the same name lands it in this fn's
        // `escape_captured_lets` too, and the missing filter emitted
        // a spurious release — the box freed while the outer frame
        // and env still held stakes, so every top-level read after
        // the call answered freed-memory garbage (rotation 162
        // capture-box UAF; test262 defineProperty toStringAccessed
        // appeared pair).
        let mut boxed: Vec<ValueId> = self
            .locals
            .iter()
            .filter(|(name, info)| {
                info.ty.is_copy() && !info.borrowed && self.escape_captured_lets.contains(*name)
            })
            .map(|(_, info)| info.slot)
            .collect();
        boxed.sort_by_key(|slot| std::cmp::Reverse(slot.0));
        for slot in boxed {
            self.f.append_void(
                self.cur_block,
                InstKind::Call(self.intrinsics.capture_box_drop, vec![Operand::Value(slot)]),
            );
        }
        // RFC 20260710 — promoted mutable non-Copy captures hold a
        // box stake too; the heap variant releases the boxed content
        // on the last drop. Borrowed entries are closure-body
        // preamble bindings — the env owns their stake, not this
        // frame.
        let mut boxed_heap: Vec<(ValueId, Type)> = self
            .locals
            .iter()
            .filter(|(name, info)| !info.borrowed && self.boxed_noncopy_lets.contains(*name))
            .map(|(_, info)| (info.slot, info.ty))
            .collect();
        boxed_heap.sort_by_key(|&(slot, _)| std::cmp::Reverse(slot.0));
        for (slot, ty) in boxed_heap {
            let fid = self.capture_box_drop_fid(ty);
            self.f.append_void(
                self.cur_block,
                InstKind::Call(fid, vec![Operand::Value(slot)]),
            );
        }
    }

    /// K.4 — drop refcount-typed module data globals at the
    /// fall-through `main` exit so the heap doesn't leak. Iterated in
    /// sorted name order for deterministic codegen across runs.
    /// Throw-out-of-main exits skip this (process abort cleans up the
    /// heap; emitting drops on an unwind path would need finally-style
    /// glue that's out of scope for K.4). Only fires inside the
    /// synthesized `main` fn.
    pub(crate) fn emit_drops_for_globals(&mut self) {
        if !self.is_main_fn {
            return;
        }
        let mut entries: Vec<(String, Type)> = self
            .globals
            .iter()
            .filter(|(_, ty)| ty.is_refcounted())
            .map(|(n, t)| (n.clone(), *t))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, ty) in entries {
            let class_cell =
                ty == Type::Any && crate::ssa_lower_closure::class_sentinel_name(self, &name);
            let ptr =
                self.f
                    .append_inst(self.cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(ptr), 0),
                ty,
                None,
            );
            // r503 — a closure-captured class cell lives here as a
            // global; same seam as the local case above.
            if class_cell {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.class_cell_release, vec![Operand::Value(v)]),
                );
            } else {
                self.emit_drop_value(Operand::Value(v), ty);
            }
        }
    }
}
