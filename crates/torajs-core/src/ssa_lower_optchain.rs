//! `Expr::OptChain` Type::Any receiver — NaN-box tag-discriminated
//! nullish short-circuit. Split from `ssa_lower.rs`'s `lower_expr_inner`
//! so the file-size known-debt only shrinks (the Type::Obj(sid) static
//! struct branch stays inline at the OptChain arm; only the Any path
//! lives here).
//!
//! Spec §13.3.9 — `obj?.field` short-circuits to `undefined` when
//! `obj == null || obj == undefined`. For a Type::Any receiver the
//! AnyValue's tag is read at runtime; ANY_NULL=0 / ANY_UNDEF=5 map to
//! ANY_UNDEF; otherwise dispatch to `lower_any_member_read`, which
//! handles dynobj entries / struct-cell fields / class-tag IC paths.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};

impl crate::ssa_lower::LowerCtx<'_> {
    /// Lower `obj?.field` when `obj` is Type::Any. Returns a fresh
    /// Any-box ptr typed as `Type::Any` — short-circuit branch boxes
    /// ANY_UNDEF=5 / 0; hit branch reuses the Any.Member read
    /// substrate (`lower_any_member_read`) which holds the per-tag
    /// dispatch. Chunk 717 — the hit arm records `eid` (the OptChain
    /// expression) as an owned read; the miss arm's undef box is an
    /// immediate, so a consumer's release is a no-op there and the
    /// join is uniformly droppable.
    pub(crate) fn lower_optchain_any(
        &mut self,
        eid: crate::ast::ExprId,
        obj_op: Operand,
        name: &str,
    ) -> Operand {
        let res_slot = self.alloca_in_entry(Type::Any, Some("__optchain"));
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_tag, vec![obj_op.clone()]),
            Type::I64,
            None,
        );
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let is_undef = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5)),
            Type::Bool,
            None,
        );
        let nullish = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Or,
                Operand::Value(is_null),
                Operand::Value(is_undef),
            ),
            Type::Bool,
            None,
        );
        let null_blk = self.f.add_block();
        let mem_blk = self.f.add_block();
        let after = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(nullish),
                then_blk: null_blk,
                else_blk: mem_blk,
            },
        );
        // Miss path → box ANY_UNDEF=5 / value=0.
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
        // Hit path → reuse Any.Member read substrate.
        self.cur_block = mem_blk;
        let field_box = crate::ssa_lower_any_member::lower_any_member_read(self, eid, obj_op, name);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(field_box, Operand::Value(res_slot), 0),
        );
        let mb = self.cur_block;
        self.f.set_term(mb, Terminator::Br(after));
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
