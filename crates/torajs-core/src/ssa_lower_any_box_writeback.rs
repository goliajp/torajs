//! Dynobj relocation write-back for `LowerCtx<'a>` — sibling of
//! `ssa_lower_any_box` (rotation 204 file-size split: the global-slot
//! arm pushed the host past the 500-line hard limit; body verbatim).

use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Step 7d-A — `dynobj_set` / `dynobj_define` may resize +
    /// relocate the underlying heap block (`*obj_slot` updated).
    /// The variable's AnyValue still holds the OLD ptr; if the
    /// receiver was a named Ident, reload the post-resize ptr and
    /// store it back as a fresh NaN-box `AnyValue`. NaN-box Cell
    /// encoding is `ptr as u64` (identical bits — the PtrToInt +
    /// IntToPtr cast is a no-op at LLVM IR; LTO collapses them
    /// into the same SSA value). Non-Ident receivers (e.g.
    /// `arr[i].x = v`) don't have a hoisted slot; the resize-time
    /// dangling is a follow-up patch (no current conformance
    /// fixture exercises it under the 7/8 load factor +
    /// `INITIAL_CAP=8`).
    pub(crate) fn emit_any_dynobj_writeback(
        &mut self,
        obj_ident: &Option<String>,
        dynobj_slot: ValueId,
    ) {
        let Some(name) = obj_ident else {
            return;
        };
        // Rotation 204 — a receiver bound to a module global (K.3's
        // Any-slot family: `: any`-annotated or dynobj-degraded, both
        // named-fn-visible) writes back through GlobalRef. The old
        // locals-only lookup silently returned here, so a relocating
        // define left the global slot pointing at the stale cell
        // (`Object.defineProperty(g, "c", ...)` then `g.c` read
        // undefined — probe-proven silent wrong).
        let store_slot: Operand = if let Some(info) = self.locals.get(name).copied() {
            if !matches!(info.ty, Type::Any) {
                return;
            }
            Operand::Value(info.slot)
        } else if self.globals.get(name) == Some(&Type::Any) {
            let ptr = self.f.append_inst(
                self.cur_block,
                InstKind::GlobalRef(name.to_string()),
                Type::Ptr,
                None,
            );
            Operand::Value(ptr)
        } else {
            return;
        };
        let new_dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(dynobj_slot), 0),
            Type::Ptr,
            None,
        );
        let new_dynobj_as_i64 = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(new_dynobj)),
            Type::I64,
            None,
        );
        let new_any = self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(new_dynobj_as_i64)),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_any), store_slot, 0),
        );
    }
}
