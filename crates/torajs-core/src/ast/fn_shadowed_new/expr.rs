//! The census through an expression: a `new` is checked against the
//! scopes, and an arrow function opens a function scope.

use super::super::{Expr, ExprId};
use super::{Census, Scope};

impl Census<'_> {
    pub(super) fn expr(&mut self, eid: ExprId) {
        match self.ast.get_expr(eid) {
            Expr::New {
                class_name, args, ..
            } => {
                if self.shadowed(class_name) {
                    self.out.insert(eid);
                }
                for a in args {
                    self.expr(*a);
                }
            }
            // A named function expression binds its own name in a
            // scope between the enclosing one and its body (§15.2.5).
            Expr::ArrowFn { body, params, .. } => {
                let own = self.ast.fn_expr_self_names.get(&eid);
                if let Some(n) = own {
                    self.scopes.push(Scope::from([(n.clone(), true)]));
                }
                self.defaults(params);
                self.body(body, params);
                if own.is_some() {
                    self.scopes.pop();
                }
            }
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Assign {
                target: left,
                value: right,
            }
            | Expr::Index {
                obj: left,
                index: right,
            }
            | Expr::OptIndex {
                obj: left,
                index: right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::InstanceOf {
                expr: left,
                rhs: right,
            } => {
                self.expr(*left);
                self.expr(*right);
            }
            Expr::Unary { expr, .. }
            | Expr::Member { obj: expr, .. }
            | Expr::As { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Delete { expr }
            | Expr::Spread { expr }
            | Expr::OptChain { obj: expr, .. }
            | Expr::PostIncr { target: expr, .. } => self.expr(*expr),
            Expr::Call { callee, args }
            | Expr::NewDynamic { callee, args }
            | Expr::OptCall { callee, args } => {
                self.expr(*callee);
                for a in args {
                    self.expr(*a);
                }
            }
            Expr::Array(items) => {
                for e in items {
                    self.expr(*e);
                }
            }
            Expr::Super { args } => {
                for a in args {
                    self.expr(*a);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    self.expr(*e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.expr(*cond);
                self.expr(*then_branch);
                self.expr(*else_branch);
            }
            Expr::Ident(_)
            | Expr::String(_)
            | Expr::Number(_)
            | Expr::BigInt { .. }
            | Expr::Bool(_)
            | Expr::Null
            | Expr::Uninit
            | Expr::Elision
            | Expr::Regex { .. }
            | Expr::Closure { .. }
            | Expr::This
            | Expr::NewTarget => {}
        }
    }
}
