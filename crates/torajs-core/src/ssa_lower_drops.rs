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
        let mut to_drop: Vec<(ValueId, Type)> = self
            .locals
            .iter()
            .filter(|(name, info)| {
                !info.moved && !info.ty.is_copy() && !self.stack_alloced_locals.contains(*name)
            })
            .map(|(_, info)| (info.slot, info.ty))
            .collect();
        // Drop in declaration reverse (slot ids are allocated in
        // declaration order), the textbook LIFO destruction order.
        to_drop.sort_by_key(|&(slot, _)| std::cmp::Reverse(slot.0));
        for (slot, ty) in to_drop {
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(slot), 0),
                ty,
                None,
            );
            self.emit_drop_value(Operand::Value(val), ty);
        }
        // RFC 20260705 chunk 550 fix-up — escape-promoted Copy locals
        // live in refcounted capture boxes (outer-stake protocol:
        // alloc = rc 1, each capturing env +1). Release the frame's
        // own stake so the box frees once the last capturing env
        // drops; without the env-temp release of chunk 550 the envs
        // never dropped and the box leaked instead.
        let mut boxed: Vec<ValueId> = self
            .locals
            .iter()
            .filter(|(name, info)| info.ty.is_copy() && self.escape_captured_lets.contains(*name))
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
            let ptr =
                self.f
                    .append_inst(self.cur_block, InstKind::GlobalRef(name), Type::Ptr, None);
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(ptr), 0),
                ty,
                None,
            );
            self.emit_drop_value(Operand::Value(v), ty);
        }
    }
}
