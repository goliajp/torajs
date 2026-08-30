//! `__proto__` in an object literal — the one key that is not a
//! field, carved out of `ssa_lower_dynobj_init` at the 500-line
//! boundary. The sibling stores what a field says; this one takes
//! the key that instead says what the object INHERITS from.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// §13.2.5.5 — the literal `__proto__: v` member sets
    /// [[Prototype]], never an own entry (RFC
    /// 20260717-user-proto-chain): a cell lands in the simulation
    /// slot, null marks the null-proto bit, any other value is
    /// silently ignored — exactly the Annex B setter core's contract
    /// (the fresh literal cannot be non-extensible or form a cycle,
    /// so its refusal path is unreachable). The value box is a
    /// borrow (the core takes its own stake).
    pub(crate) fn emit_dynobj_proto_field(&mut self, slot: ValueId, fval_eid: ExprId) {
        // A statically-null proto marks the header bit
        // directly (the `Object.create(null)` face) — the
        // boxed-setter path below covers runtime values.
        if matches!(self.ast.get_expr(fval_eid), Expr::Null)
            || matches!(
                self.expr_types.get(&fval_eid),
                Some(crate::check::Type::Null)
            )
        {
            let _ = self.lower_expr(fval_eid);
            let dynobj = self.load_dynobj(slot);
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.dynobj_mark_null_proto,
                    vec![Operand::Value(dynobj)],
                ),
            );
            return;
        }
        let v_op = self.lower_expr(fval_eid);
        let v_ty = self.operand_ty(&v_op);
        let v_boxed = if matches!(v_ty, Type::Any) {
            v_op.clone()
        } else {
            self.box_to_any_from_expr(fval_eid, v_op.clone())
        };
        let dynobj = self.load_dynobj(slot);
        let obj_boxed = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(4 /* ANY_HEAP */), Operand::Value(dynobj)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.anyv_proto_member_set,
                vec![Operand::Value(obj_boxed), v_boxed],
            ),
        );
        self.release_owned_temp(fval_eid, &v_op);
    }
}
