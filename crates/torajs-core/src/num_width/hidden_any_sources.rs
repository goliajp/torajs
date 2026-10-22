//! What an expression of `hidden_any` carries: the nodes whose value
//! it may hand on.
//!
//! The value flows through copies, `+` (it may concatenate), `&&`,
//! `||`, `??`, the conditional, the comma and assignment expressions.
//! Every other operator answers a number or a boolean whatever its
//! operands hold, so the flow stops there.

use super::container::canon_key_frozen;
use super::hidden_any::{Graph, Node};
use super::hidden_any_calls::answers_any;
use super::{Analysis, Scope};
use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;

impl Graph<'_> {
    pub(super) fn sources(
        &mut self,
        a: &Analysis<'_>,
        eid: ExprId,
        scope: &Scope,
        out: &mut Vec<Node>,
    ) {
        match a.ast.get_expr(eid) {
            Expr::Ident(n) => out.extend(self.resolve(a, n, scope).reads()),
            Expr::Call { .. } if answers_any(a, eid) => {
                self.seeds.push(Node::AnyVal);
                out.push(Node::AnyVal);
            }
            Expr::Call { callee, .. } => {
                out.extend(self.call_result(a, eid, *callee));
                out.extend(self.container_read(a, eid, scope));
            }
            Expr::Member { .. } => out.extend(self.container_read(a, eid, scope)),
            Expr::Index { obj, .. } | Expr::OptIndex { obj, .. } => {
                if let Some(n) = self.elem_source(a, *obj, scope) {
                    self.seeds.push(n.clone());
                    out.push(n);
                }
            }
            // a `+` the checker types `string` has a string operand and
            // answers a string whatever the other one holds
            Expr::BinOp { op: BinOp::Add, .. } if a.expr_types.get(&eid) == Some(&Type::String) => {
            }
            Expr::BinOp {
                op: BinOp::Add | BinOp::LAnd | BinOp::LOr,
                left,
                right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.sources(a, *left, scope, out);
                self.sources(a, *right, scope, out);
            }
            Expr::Ternary {
                then_branch,
                else_branch,
                ..
            } => {
                self.sources(a, *then_branch, scope, out);
                self.sources(a, *else_branch, scope, out);
            }
            Expr::Sequence { right: e, .. }
            | Expr::As { expr: e, .. }
            | Expr::Assign { value: e, .. } => self.sources(a, *e, scope, out),
            _ => {}
        }
    }

    /// The source a read of an element of `obj` is: any element of an
    /// `any[]`, or the element point of a typed array whose alias class
    /// escaped, which answers whatever the `any` side stored.
    fn elem_source(&self, a: &Analysis<'_>, obj: ExprId, scope: &Scope) -> Option<Node> {
        let Some(Type::Array(el)) = a.expr_types.get(&obj) else {
            return None;
        };
        if **el == Type::Any {
            return Some(Node::AnyVal);
        }
        let k = canon_key_frozen(&a.uf, &a.keys, a.container_key_lookup(obj, scope)?);
        self.any_escaped
            .contains(&k)
            .then(|| Node::Read(a.keys.elem(k)))
    }
}
