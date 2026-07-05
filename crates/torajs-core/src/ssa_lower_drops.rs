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
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
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
