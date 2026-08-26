//! Closure `fn.props` accessor helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 372.
//!
//! T-27 — `f.x = (tag, val)` against a closure: drives the lazy
//! `props_dynobj` slot at `CLOSURE_PROPS_OFF`, allocating on first
//! write with a resize-aware writeback. The read twin (`fn_props_get`,
//! an own-expando-only probe) retired in r505: a fn-as-object read
//! takes the any-member lane, whose closure arm walks the whole
//! inheritance ladder (see `ssa_lower_member`'s Closure arm).

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{CLOSURE_PROPS_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// T-27 — `f.x = (tag, val)` against a closure. Loads the lazy
    /// props_dynobj at CLOSURE_PROPS_OFF, allocates it on first write,
    /// stores the new ptr back into the closure (it may also resize
    /// later). Then calls dynobj_set against the live ptr through a
    /// stack slot so the resize-aware writeback works.
    pub(crate) fn fn_props_set(
        &mut self,
        closure_op: Operand,
        key: &str,
        tag: Operand,
        val_op: Operand,
    ) {
        let key_str = self.intern_string_literal(key);
        // Load current props ptr (NULL on first write).
        let cur_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        // Branch on NULL: alloc-and-store, or use existing.
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(cur_props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let alloc_blk = self.f.add_block();
        let after_alloc = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: alloc_blk,
                else_blk: after_alloc,
            },
        );
        // alloc path: dynobj_alloc() → store at CLOSURE_PROPS_OFF.
        self.cur_block = alloc_blk;
        let new_props = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, vec![]),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(new_props),
                closure_op.clone(),
                CLOSURE_PROPS_OFF,
            ),
        );
        let ab = self.cur_block;
        self.f.set_term(ab, Terminator::Br(after_alloc));
        // after_alloc: re-load to get whichever path's value, stash in
        // a stack slot for the resize-aware dynobj_set, set, then write
        // back to closure props.
        self.cur_block = after_alloc;
        let live_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let slot = self.alloca(Type::Ptr, Some("__fnprops_slot"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(live_props), Operand::Value(slot), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_set,
                vec![Operand::Value(slot), Operand::Value(key_str), tag, val_op],
            ),
        );
        // P3.attribute-flag-tracking — fnprops user assign can hit a
        // writable=false existing bucket.
        self.emit_throw_check(None);
        // Writeback resize-aware ptr.
        let new_live = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_live), closure_op, CLOSURE_PROPS_OFF),
        );
    }
}
