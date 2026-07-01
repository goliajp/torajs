//! Closure `fn.props` accessor helpers for `LowerCtx<'a>` extracted
//! from `ssa_lower.rs` chunk 372.
//!
//! T-27 — `f.x = (tag, val)` / `f.x` read against a closure. Both
//! methods drive the lazy `props_dynobj` slot at `CLOSURE_PROPS_OFF`:
//! `fn_props_set` allocates on first write + resize-aware writeback,
//! `fn_props_get` returns `ANY_UNDEF` when the slot is NULL. Method
//! bodies are byte-for-byte preserved from the source; siblings and
//! `ssa_lower.rs` reach them through the impl block on the shared
//! `crate::ssa_lower::LowerCtx` type.

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

    /// T-27 — `f.x` read against a closure. Loads props_dynobj at
    /// CLOSURE_PROPS_OFF; NULL → ANY_UNDEF box. Otherwise calls
    /// dynobj_get_tag/value with the key and boxes the result.
    pub(crate) fn fn_props_get(&mut self, closure_op: Operand, key: &str) -> Operand {
        let key_str = self.intern_string_literal(key);
        let res_slot = self.alloca_in_entry(Type::Any, Some("__fnprops_get"));
        let props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op, CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let null_blk = self.f.add_block();
        let read_blk = self.f.add_block();
        let after = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: null_blk,
                else_blk: read_blk,
            },
        );
        // null path: undef box.
        self.cur_block = null_blk;
        let undef_box = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            null_blk,
            InstKind::Store(Operand::Value(undef_box), Operand::Value(res_slot), 0),
        );
        self.f.set_term(null_blk, Terminator::Br(after));
        // read path: dynobj_get_tag/value + box.
        self.cur_block = read_blk;
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_tag,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let value = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_value,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let box_v = crate::ssa_lower_accessor::emit_dynobj_get_result(self, tag, value);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(box_v.clone(), Operand::Value(res_slot), 0),
        );
        let rb = self.cur_block;
        self.f.set_term(rb, Terminator::Br(after));
        self.cur_block = after;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
            Type::Any,
            None,
        );
        Operand::Value(r)
    }
}
