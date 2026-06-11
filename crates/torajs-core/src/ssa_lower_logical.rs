//! Short-circuit `&&` / `||` lowering (extracted from `ssa_lower.rs`,
//! file-size known-debt #1).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// M1.5 — `a && b` with short-circuit. Layout:
    ///
    /// ```text
    ///   <slot> = alloca bool
    ///   av = lower(a)
    ///   cond_br av, eval_b, false_blk
    /// eval_b:
    ///   bv = lower(b)
    ///   store bv → slot
    ///   br merge
    /// false_blk:
    ///   store false → slot
    ///   br merge
    /// merge:
    ///   load slot
    /// ```
    /// V3-18 m1.g — JS spec §13.13: `a && b` returns `a` if it's
    /// falsy, otherwise `b`. Result type is the common type of
    /// both operands (typed tora gates on l == r at typecheck;
    /// implicit-any (m1.h) widens to mixed types later).
    pub(crate) fn lower_logical_and(&mut self, left: ExprId, right: ExprId) -> Operand {
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a);
        let slot = self.alloca(a_ty, None);
        let eval_b = self.f.add_block();
        let false_blk = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: truthy,
                then_blk: eval_b,
                else_blk: false_blk,
            },
        );
        self.cur_block = eval_b;
        let b = self.lower_expr(right);
        self.f
            .append_void(self.cur_block, InstKind::Store(b, Operand::Value(slot), 0));
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = false_blk;
        // a is the falsy value — return it directly (matches JS:
        // `0 && expr` returns 0, not false; `"" && expr` returns "").
        self.f
            .append_void(self.cur_block, InstKind::Store(a, Operand::Value(slot), 0));
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(a_ty, Operand::Value(slot), 0),
            a_ty,
            None,
        );
        Operand::Value(v)
    }

    /// V3-18 m1.g — JS spec §13.13: `a || b` returns `a` if truthy,
    /// otherwise `b`. Mirror of `&&`.
    pub(crate) fn lower_logical_or(&mut self, left: ExprId, right: ExprId) -> Operand {
        let a = self.lower_expr(left);
        let a_ty = self.operand_ty(&a);
        let truthy = self.coerce_to_bool(a);
        let slot = self.alloca(a_ty, None);
        let true_blk = self.f.add_block();
        let eval_b = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: truthy,
                then_blk: true_blk,
                else_blk: eval_b,
            },
        );
        self.cur_block = true_blk;
        // a is truthy — return it directly (matches JS: `5 || 0`
        // returns 5; `"x" || ""` returns "x").
        self.f
            .append_void(self.cur_block, InstKind::Store(a, Operand::Value(slot), 0));
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = eval_b;
        let b = self.lower_expr(right);
        self.f
            .append_void(self.cur_block, InstKind::Store(b, Operand::Value(slot), 0));
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(a_ty, Operand::Value(slot), 0),
            a_ty,
            None,
        );
        Operand::Value(v)
    }
}
