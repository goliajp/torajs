//! The functions a value of the return walk may be, whose returns a
//! call of it answers.

use std::collections::HashSet;

use super::traced;
use crate::ast::{BinOp, Expr, ExprId};
use crate::check::Type;
use crate::num_width::hidden_any::{Graph, Node, Ref};
use crate::num_width::{Analysis, Scope, SlotKey};

impl Graph<'_> {
    /// Every value a direct call of `f` hands its parameter `p`, and
    /// every value stored to the name; `None` when some call of `f` is
    /// out of sight.
    fn param_stores(&self, a: &Analysis<'_>, f: &str, p: &str) -> Option<Vec<ExprId>> {
        let l = self.lookups(a);
        // a closure, a method or a function also read as a value is
        // called from out of sight
        if f.starts_with("__") || l.fn_values.contains(f) {
            return None;
        }
        let i = l.user_params.get(f)?.iter().position(|q| q == p)?;
        let calls = l.call_args.get(f).into_iter().flatten();
        let mut out: Vec<ExprId> = calls.filter_map(|args| args.get(i).copied()).collect();
        out.extend(l.stores.get(p).into_iter().flatten());
        Some(out)
    }

    /// The returns of the functions call `eid` of `callee` may run, or
    /// `None` when one of them is out of sight.
    pub(super) fn callees(
        &self,
        a: &Analysis<'_>,
        sc: &Scope,
        eid: ExprId,
        callee: ExprId,
    ) -> Option<Vec<Node>> {
        if let Some(Node::Ret(f)) = self.call_result(a, eid, callee) {
            return traced(&f).then(|| vec![Node::RetAny(f)]);
        }
        let mut out = Vec::new();
        let seen = self.fn_values(a, sc, callee, &mut out, &mut HashSet::new());
        let traced = out
            .iter()
            .all(|n| matches!(n, Node::RetAny(f) if traced(f)));
        (seen && traced).then_some(out)
    }

    /// Push the return of each function the value `e` may be; false
    /// when it may be one out of sight.
    fn fn_values(
        &self,
        a: &Analysis<'_>,
        sc: &Scope,
        e: ExprId,
        out: &mut Vec<Node>,
        seen: &mut HashSet<ExprId>,
    ) -> bool {
        if !seen.insert(e) {
            return true;
        }
        // a value that is no function answers no call (`let f: any;`
        // before its stores, `undefined` for a callback left out)
        let callable = matches!(
            a.expr_types.get(&e),
            None | Some(Type::Function(..) | Type::Any | Type::Nullable(_))
        );
        if !callable || matches!(a.ast.get_expr(e), Expr::Uninit | Expr::Null) {
            return true;
        }
        let mut go = |x: ExprId, out: &mut Vec<Node>| self.fn_values(a, sc, x, out, seen);
        match a.ast.get_expr(e) {
            Expr::Closure { fn_name, .. } => {
                out.push(Node::RetAny(fn_name.clone()));
                true
            }
            Expr::Ident(n) => match self.resolve(a, n, sc) {
                Ref::At(Node::Decl(i)) => match self.values(a, n, self.origin(i)) {
                    Some(vs) => vs.into_iter().fold(true, |h, v| go(v, out) & h),
                    None => false,
                },
                Ref::At(Node::Read(SlotKey::Param(f, p))) if declared_face(a, e, &f, &p) => true,
                // the arguments sit in the callers' scopes: only a value
                // that names no binding is read there
                Ref::At(Node::Read(SlotKey::Param(f, p))) => match self.param_stores(a, &f, &p) {
                    Some(stores) => stores.into_iter().all(|s| scope_free(a, s, out)),
                    None => false,
                },
                _ if a.fn_params.contains_key(n) && !a.shadowing_reads.contains(&e) => {
                    out.push(Node::RetAny(n.clone()));
                    true
                }
                Ref::Keyed(_) => {
                    let origins = self.lookups(a).decls.get(n).cloned().unwrap_or_default();
                    let vs: Option<Vec<ExprId>> = (origins.into_iter())
                        .map(|o| self.values(a, n, o))
                        .collect::<Option<Vec<_>>>()
                        .map(|v| v.concat());
                    vs.is_some_and(|vs| {
                        !vs.is_empty() && vs.into_iter().fold(true, |h, v| go(v, out) & h)
                    })
                }
                _ => false,
            },
            Expr::BinOp {
                op: BinOp::LAnd | BinOp::LOr,
                left,
                right,
            }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            }
            | Expr::Ternary {
                then_branch: left,
                else_branch: right,
                ..
            } => go(*left, out) & go(*right, out),
            Expr::Sequence { right: x, .. } => go(*x, out),
            _ => false,
        }
    }
}

/// Push the return of the function `e` is when it names no binding: a
/// closure or a declared function; a value that is no function answers
/// no call.
fn scope_free(a: &Analysis<'_>, e: ExprId, out: &mut Vec<Node>) -> bool {
    match a.ast.get_expr(e) {
        Expr::Closure { fn_name, .. } => out.push(Node::RetAny(fn_name.clone())),
        Expr::Ident(g) if a.fn_params.contains_key(g) && !a.shadowing_reads.contains(&e) => {
            out.push(Node::RetAny(g.clone()))
        }
        Expr::Uninit | Expr::Null => {}
        _ => {
            return !matches!(
                a.expr_types.get(&e),
                None | Some(Type::Function(..) | Type::Any | Type::Nullable(_))
            );
        }
    }
    true
}

/// Does parameter `p` of `f`, read at `e`, only hold functions of its
/// declared face, which answers a typed value? A parameter some call
/// hands a function of another face is in the boxed-entry census.
fn declared_face(a: &Analysis<'_>, e: ExprId, f: &str, p: &str) -> bool {
    let face = match a.expr_types.get(&e) {
        Some(Type::Nullable(inner)) => inner.as_ref(),
        Some(t) => t,
        None => return false,
    };
    matches!(face, Type::Function(_, r) if **r != Type::Any)
        && !a.ast.boxed_params_of(f).is_some_and(|ps| ps.contains(p))
}
