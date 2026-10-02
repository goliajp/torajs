//! Per-store header-bit branches on the typed element-assign path —
//! the §10.4.2.1 integrity refusal (before the slot is written) and
//! the hole revive (after). Split out of `ssa_lower_index_assign.rs`
//! under the 500-line file rule when the integrity gate landed.
//!
//! Both read the SAME header word, and both are shaped so a plain
//! array pays one predictable not-taken branch: the flags that can
//! make an index special all live in the top u16 of the header, so
//! one mask decides whether the slow block runs at all.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_index_assign::WriteBack;

impl<'a> LowerCtx<'a> {
    /// §10.4.2.1 — refuse a store into a non-writable index (an
    /// explicit `writable: false` shadow, or any index of a frozen
    /// array) before the slot is written. The `any`-receiver lane
    /// gates inside its kernel; the typed tier stores straight into
    /// the slot, so it needs the gate emitted.
    ///
    /// A plain array pays one predictable not-taken branch: the four
    /// header bits that can make an index non-writable
    /// (defineProperty shadow / frozen / sealed / non-extensible) test
    /// as ONE mask over the same header word the hole-revive branch
    /// below already loads, and only a hit calls the kernel.
    pub(crate) fn emit_index_integrity_guard(&mut self, arr_val: &Operand, idx_val: &Operand) {
        // Header word is `[refcount u32 | tag u16 | flags u16]`, so a
        // flags bit N sits at bit 48+N of the I64 load.
        const FLAGS_SHIFT: u32 = 48;
        let mask = ((torajs_rc::FLAG_ARR_EXOTIC_INDEX
            | torajs_rc::FLAG_FROZEN
            | torajs_rc::FLAG_SEALED
            | torajs_rc::FLAG_NON_EXTENSIBLE) as i64)
            << FLAGS_SHIFT;
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, *arr_val, 0),
            Type::I64,
            None,
        );
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                crate::ssa::BinOp::And,
                Operand::Value(hdr),
                Operand::ConstI64(mask),
            ),
            Type::I64,
            None,
        );
        let guarded = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let check_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        let cur = self.cur_block;
        self.f.set_term(
            cur,
            Terminator::CondBr {
                cond: Operand::Value(guarded),
                then_blk: check_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = check_blk;
        self.f.append_void(
            check_blk,
            InstKind::Call(
                self.intrinsics.arr_index_check_store,
                vec![*arr_val, *idx_val],
            ),
        );
        // The kernel ARMS the TypeError (the `obj_check_not_frozen`
        // shape); this diverts to the user's handler before the slot
        // is written.
        self.emit_throw_check(None);
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(cont_blk));
        self.cur_block = cont_blk;
    }

    pub(crate) fn emit_hole_revive_branch(
        &mut self,
        arr_val: &Operand,
        idx_val: &Operand,
        join_blk: BlockId,
    ) {
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, *arr_val, 0),
            Type::I64,
            None,
        );
        let exotic_bit = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                crate::ssa::BinOp::And,
                Operand::Value(hdr),
                Operand::ConstI64(i64::MIN),
            ),
            Type::I64,
            None,
        );
        let is_exotic = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(exotic_bit), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let revive_blk = self.f.add_block();
        let wb = self.cur_block;
        self.f.set_term(
            wb,
            Terminator::CondBr {
                cond: Operand::Value(is_exotic),
                then_blk: revive_blk,
                else_blk: join_blk,
            },
        );
        self.cur_block = revive_blk;
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_index_revive_idx,
                vec![*arr_val, *idx_val],
            ),
        );
        let rb = self.cur_block;
        self.f.set_term(rb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
    }

    /// Rotation 623 — the typed store's arm for an index that is not
    /// an integer: `xs[1.5] = v` names the property "1.5" (§7.1.19),
    /// and the keyed kernel makes that expando the way it does for an
    /// `any` receiver. Entered from `lower_index_assign`'s flag branch
    /// with the value already lowered (evaluation order); leaves
    /// `cur_block` where the typed arm's join left it.
    pub(crate) fn emit_nonintegral_index_expando_store(
        &mut self,
        expando_blk: BlockId,
        raw_key: Operand,
        eid: ExprId,
        value: ExprId,
        v_raw: &Operand,
        arr_val: &Operand,
    ) {
        let after = self.cur_block;
        self.cur_block = expando_blk;
        let v_ty = self.operand_ty(v_raw);
        let (tag_op, value_op) = self.pack_any_slot_value_shared(value, v_raw, v_ty);
        crate::ssa_lower_index_assign::mint_index_assign_value(self, eid, v_raw);
        let boxed_arr = self.box_to_any(*arr_val);
        let boxed_key = self.box_to_any(raw_key);
        crate::ssa_lower_index_any_key::AnyNumericKey::Boxed(boxed_key).emit_set(
            self,
            boxed_arr,
            tag_op,
            value_op,
            Operand::ConstPtrNull,
        );
        self.emit_throw_check(None);
        let eb = self.cur_block;
        self.f.set_term(eb, Terminator::Br(after));
        self.cur_block = after;
    }

    /// The store's split on a "not an integer" index (`lower_typed
    /// _read_index`): opens the expando block and leaves `cur_block`
    /// on the element-store block; the caller stores, branches to
    /// the join, and hands the expando block to
    /// `emit_nonintegral_index_expando_store`.
    pub(crate) fn emit_nonintegral_store_split(
        &mut self,
        nonint: Option<(Operand, Operand)>,
    ) -> Option<(BlockId, BlockId, Operand)> {
        let (flag, raw) = nonint?;
        let expando_blk = self.f.add_block();
        let store_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: flag,
                then_blk: expando_blk,
                else_blk: store_blk,
            },
        );
        self.cur_block = store_blk;
        Some((expando_blk, join_blk, raw))
    }

    /// bug-327 C3 — the receiver's write-back slot (the two shapes
    /// arr_push supports): present, the growable helper; absent, the
    /// plain entry whose OOB path is a loud RangeError.
    pub(crate) fn resolve_index_writeback(&self, obj: ExprId) -> Option<WriteBack> {
        if let Expr::Ident(name) = self.ast.get_expr(obj) {
            if self.locals.contains_key(name) {
                Some(WriteBack::Local)
            } else if self.globals.contains_key(name) {
                Some(WriteBack::Global)
            } else {
                None
            }
        } else {
            None
        }
    }
}
