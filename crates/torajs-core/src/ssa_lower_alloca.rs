//! Alloca helpers for `LowerCtx<'a>` extracted from `ssa_lower.rs`
//! chunk 376.
//!
//! Four stack-slot allocators used throughout SSA lowering: `alloca`
//! (current-block slot, cheap default for Copy locals), `alloca_in_entry`
//! (entry-block slot for values whose loads span multiple non-entry
//! predecessors — pending_break/continue flags, refcounted binding slots),
//! `binding_slot_alloca` (LetDecl slot picker that entry-hoists +
//! NULL-inits refcounted slots for correct drop behavior across throw /
//! finally paths), and `alloca_bool_flag_in_entry` (entry-hoisted Bool
//! slot pre-initialized to `false` for pending-break / pending-continue
//! flags). Method bodies are byte-for-byte preserved from the source;
//! the sibling reaches LowerCtx fields via
//! `impl<'a> super::LowerCtx<'a>`, so call sites need zero edits.

use crate::ssa::{BlockId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Allocate a stack slot of `ty` in the current block. Returns the
    /// alloca's pointer ValueId. Used for `let`-decl locals + parameter
    /// home-slots (see lower_fn).
    pub(crate) fn alloca(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(self.cur_block, InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// Allocate in the function's entry block (BlockId(0)) regardless of
    /// where lowering is currently positioned. Needed for slots whose
    /// loads happen on multiple control-flow predecessors that share no
    /// dominator other than entry — e.g. `__pending_break` /
    /// `__pending_continue` flags, where the lazy alloca otherwise lands
    /// in the break-block (which doesn't dominate the finally-tail
    /// fall-through path) and LLVM rejects with "Instruction does not
    /// dominate all uses".
    pub(crate) fn alloca_in_entry(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(BlockId(0), InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// LetDecl binding-slot allocation. A refcounted binding is dropped
    /// at scope end, and that drop's `load <slot>` can land in a block
    /// with multiple predecessors sharing no dominator but entry (e.g.
    /// the post-try continuation, reachable from both the try-normal
    /// and catch paths). If the slot were `alloca`'d in whatever block
    /// lowering happened to be in — which a mid-expression block split
    /// (a may-throw call / bigint op) moves forward — that block won't
    /// dominate the drop's load and codegen rejects ("unmapped SSA
    /// value" — the backend maps values in block-insertion order).
    /// Entry-hoisting refcounted slots is the standard LLVM shape (all
    /// allocas in entry; mem2reg promotes them) and removes the whole
    /// fragility class. Copy slots have no scope-end drop, so they keep
    /// the cheaper in-place alloca (no behavior change for them).
    pub(crate) fn binding_slot_alloca(&mut self, ty: Type, name: &str) -> ValueId {
        if ty.is_refcounted() {
            let slot = self.alloca_in_entry(ty, Some(name));
            // T-49b — NULL-init the refcounted slot at entry. Without
            // this, a `const c = <may-throw-expr>` whose RHS throws
            // (e.g. `10n / 0n`) leaves the entry-hoisted slot with
            // stack-uninit bytes; the scope-end / main-exit drop walk
            // then calls `rc_dec` on garbage and SIGSEGVs. NULL is
            // the rc-dec NULL-guard sentinel — drops on it are
            // no-ops, mirroring the OLD LLVM pipeline's behaviour
            // where the LLVM mem2pass turns the alloca into an SSA
            // phi initialized to `null` in the entry block.
            //
            // Cheap: one store per refcounted let / const binding,
            // overwritten by the normal-path assignment. Bool flags
            // already follow this shape (see
            // `alloca_bool_flag_in_entry`).
            self.f.append_void(
                BlockId(0),
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
            );
            slot
        } else if self.case_block_depth > 0 {
            // Inside a switch CaseBlock the frame spans sibling clause
            // blocks, so the declaration's own block does not dominate
            // a read from another clause — see `case_block_depth`.
            self.alloca_in_entry(ty, Some(name))
        } else {
            self.alloca(ty, Some(name))
        }
    }

    /// Same as `alloca_in_entry` but also seeds the slot with `false`
    /// (for Bool flags) in the entry block. Without this, the flag is
    /// uninitialized memory on paths that reach the finally tail without
    /// having taken the break/continue branch (e.g. the i=0 iteration
    /// of `for { try { if i===N break } finally { … } }`); the finally
    /// tail's `Load` then sees garbage and may spuriously route through
    /// the break dispatch on the very first pass.
    pub(crate) fn alloca_bool_flag_in_entry(&mut self, name: Option<&str>) -> ValueId {
        let slot = self.alloca_in_entry(Type::Bool, name);
        self.f.append_void(
            BlockId(0),
            InstKind::Store(Operand::ConstBool(false), Operand::Value(slot), 0),
        );
        slot
    }
}
