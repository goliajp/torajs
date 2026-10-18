//! The expressions of the view scan, in evaluation order: which reads
//! of the view they make, and which of them can move the buffer.

use crate::ast::desugar_async_sm_walk::visit_expr_children;
use crate::ast::{Expr, ExprId};
use crate::check::Type;

use super::super::program::Cat;
use super::Scan;

impl Scan<'_, '_> {
    pub(super) fn expr(&mut self, e: ExprId) {
        let ast = self.prog.ast;
        match ast.get_expr(e) {
            Expr::Ident(n) => {
                if n == self.name && self.h.cat_of(self.prog, e) == Some(Cat::Elem) {
                    self.reads += 1;
                    self.stale |= self.hazard;
                }
            }
            Expr::Number(_)
            | Expr::String(_)
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Uninit
            | Expr::BigInt { .. }
            | Expr::This
            | Expr::Closure { .. }
            | Expr::ArrowFn { .. } => {}
            Expr::Member { obj, .. } => {
                self.expr(*obj);
                if !self.plain_member(*obj) {
                    self.set_hazard();
                }
            }
            Expr::Index { obj, index } => {
                self.expr(*obj);
                self.expr(*index);
                if !self.plain_recv(*obj) || !self.primitive(*index) {
                    self.set_hazard();
                }
            }
            Expr::Call { callee, args } => {
                let own_factory = matches!(ast.get_expr(*callee),
                    Expr::Ident(n) if n.strip_prefix("__new_") == Some(self.h.class));
                if let Expr::Member { obj, .. } = ast.get_expr(*callee) {
                    self.expr(*obj);
                } else if !own_factory {
                    self.expr(*callee);
                }
                args.iter().for_each(|a| self.expr(*a));
                if !own_factory {
                    self.set_hazard();
                }
            }
            Expr::New {
                class_name, args, ..
            } => {
                args.iter().for_each(|a| self.expr(*a));
                if class_name != self.h.class {
                    self.set_hazard();
                }
            }
            Expr::Assign { target, value } => {
                visit_expr_children(ast, *target, &mut |c| self.expr(c));
                self.expr(*value);
                if !self.plain_store(e, *target) {
                    self.set_hazard();
                }
            }
            Expr::PostIncr { target, .. } => {
                visit_expr_children(ast, *target, &mut |c| self.expr(c));
                if !self.plain_store(e, *target) || !self.primitive(e) {
                    self.set_hazard();
                }
            }
            Expr::BinOp { left, right, .. } => {
                self.expr(*left);
                self.expr(*right);
                if !self.primitive(*left) || !self.primitive(*right) {
                    self.set_hazard();
                }
            }
            Expr::Unary { expr, .. } | Expr::TypeOf { expr } => {
                self.expr(*expr);
                if !self.primitive(*expr) {
                    self.set_hazard();
                }
            }
            Expr::Ternary { .. } | Expr::Sequence { .. } | Expr::Nullish { .. } => {
                visit_expr_children(ast, e, &mut |c| self.expr(c));
            }
            _ => {
                visit_expr_children(ast, e, &mut |c| self.expr(c));
                self.set_hazard();
            }
        }
    }

    /// A receiver whose element reads are plain loads.
    fn plain_recv(&self, e: ExprId) -> bool {
        matches!(
            self.prog.ty(e),
            Some(Type::ClassRef(_) | Type::Array(_) | Type::String)
        )
    }

    /// A member read that is a plain load: no accessor anywhere in the
    /// program, and a receiver whose layout is known.
    fn plain_member(&self, obj: ExprId) -> bool {
        !self.prog.accessors && self.plain_recv(obj)
    }

    /// A store `target = ..` that runs no user code and touches no
    /// slot holding an instance.
    fn plain_store(&self, assign: ExprId, target: ExprId) -> bool {
        if self.h.cat_of(self.prog, assign).is_some() {
            return false;
        }
        match self.prog.ast.get_expr(target) {
            Expr::Ident(_) => true,
            Expr::Member { obj, name } => {
                !self.h.holding_fields.contains(name.as_str()) && self.plain_member(*obj)
            }
            _ => false,
        }
    }

    fn primitive(&self, e: ExprId) -> bool {
        matches!(
            self.prog.ty(e),
            Some(
                Type::Number
                    | Type::Boolean
                    | Type::String
                    | Type::BigInt
                    | Type::Null
                    | Type::Undefined
                    | Type::Literal(_)
            )
        )
    }
}
