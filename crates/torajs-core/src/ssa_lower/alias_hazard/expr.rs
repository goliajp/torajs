//! The expression half of the walk.
//!
//! Operands are evaluated left to right, and a value handed on — an
//! argument, an array or object-literal element, a spread, a method
//! receiver — is only taken by its consumer once every later operand
//! has run. So a binding passed directly and a release in a later
//! operand (or in the call itself) is a read after release, even though
//! the binding's own read came first.

use crate::ast::desugar_async_sm_walk::visit_expr_children;
use crate::ast::{BinOp, Expr, ExprId, UnaryOp};

use super::scan::Scan;

impl Scan<'_, '_> {
    pub(super) fn expr(&mut self, e: ExprId) {
        let ast = self.env.ast;
        match ast.get_expr(e) {
            Expr::Ident(n) => {
                if n == self.name {
                    self.read();
                }
            }
            Expr::Closure { captures, .. } => {
                if captures.iter().any(|c| c == self.name) {
                    self.read();
                    self.captured = true;
                }
            }
            Expr::ArrowFn { body, .. } => {
                if super::mentions::list(ast, body, self.name) {
                    self.read();
                    self.captured = true;
                }
            }
            Expr::Assign { target, value } => {
                match ast.get_expr(*target) {
                    Expr::Ident(_) => {}
                    _ => visit_expr_children(ast, *target, &mut |c| self.expr(c)),
                }
                self.expr(*value);
                if self.store_releases(*target) {
                    self.release();
                }
            }
            Expr::PostIncr { target, .. } => {
                match ast.get_expr(*target) {
                    Expr::Ident(n) => {
                        if n == self.name {
                            self.read();
                        }
                    }
                    _ => visit_expr_children(ast, *target, &mut |c| self.expr(c)),
                }
                if self.store_releases(*target) {
                    self.release();
                }
            }
            Expr::Call { .. }
            | Expr::OptCall { .. }
            | Expr::New { .. }
            | Expr::NewDynamic { .. }
            | Expr::Super { .. } => {
                let pending = self.operands(e);
                if !self.call_is_inert(e) {
                    if pending {
                        self.unsafe_read = true;
                    }
                    self.release();
                }
            }
            Expr::Array(_) | Expr::ObjectLit { .. } => {
                self.operands(e);
            }
            _ => {
                visit_expr_children(ast, e, &mut |c| self.expr(c));
                if self.runs_user_code(e) {
                    self.release();
                }
            }
        }
    }

    /// Walk a consumer's operands in order. Answers whether one of them
    /// handed the binding on, still untaken when the consumer runs.
    fn operands(&mut self, e: ExprId) -> bool {
        let ast = self.env.ast;
        let callee = match ast.get_expr(e) {
            Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => Some(*callee),
            _ => None,
        };
        let mut pending = false;
        visit_expr_children(ast, e, &mut |c| {
            let before = self.released;
            self.expr(c);
            if pending && !before && self.released {
                self.unsafe_read = true;
            }
            pending |= if Some(c) == callee {
                // a method call hands its receiver on
                match ast.get_expr(c) {
                    Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.hands_on(*obj),
                    _ => false,
                }
            } else {
                self.hands_on(c)
            };
        });
        pending
    }

    /// The operand's value is the binding's own value or a cell borrowed
    /// through it (`old.child`, `old[0]`), passed on as is. A value read
    /// out as a number or a string is a copy and hands nothing on.
    fn hands_on(&self, c: ExprId) -> bool {
        match self.env.ast.get_expr(c) {
            Expr::Ident(n) => n == self.name,
            Expr::As { expr, .. } | Expr::Spread { expr } => self.hands_on(*expr),
            Expr::Sequence { right, .. } => self.hands_on(*right),
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => self.hands_on(*then_branch) || self.hands_on(*else_branch),
            Expr::Nullish { lhs, rhs }
            | Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left: lhs,
                right: rhs,
            } => self.hands_on(*lhs) || self.hands_on(*rhs),
            Expr::Member { obj, .. }
            | Expr::OptChain { obj, .. }
            | Expr::Index { obj, .. }
            | Expr::OptIndex { obj, .. } => !self.primitive_operand(c) && self.hands_on(*obj),
            _ => false,
        }
    }

    /// Operators and reads that may reach user code on their own: a
    /// conversion of an object operand (`valueOf` / `toString` /
    /// `@@toPrimitive`), `@@hasInstance`, an iterator step, a getter, a
    /// keyed read off anything but an array or a string.
    fn runs_user_code(&self, e: ExprId) -> bool {
        match self.env.ast.get_expr(e) {
            Expr::BinOp { op, left, right } => {
                !matches!(op, BinOp::Eq | BinOp::Neq | BinOp::LAnd | BinOp::LOr)
                    && !(self.primitive_operand(*left) && self.primitive_operand(*right))
            }
            Expr::Unary { op, expr } => {
                !matches!(op, UnaryOp::Not) && !self.primitive_operand(*expr)
            }
            // a deleted property's value is released with it
            Expr::InstanceOf { .. } | Expr::Delete { .. } => true,
            Expr::Spread { expr } => {
                !self.indexable(*expr) || self.env.shadow.iterator_hook_written()
            }
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                self.read_may_run_getter(e, name)
            }
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                !(self.indexable(*obj) && self.primitive_operand(*index))
            }
            _ => false,
        }
    }
}
