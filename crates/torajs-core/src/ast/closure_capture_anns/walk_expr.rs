//! The expression half of the construction-site walk: find every
//! closure and method-carrying object literal an expression mints and
//! snapshot the binds in force there.

use super::super::{AstExprsView, Expr, ExprId};
use super::{Binds, Ctx};

impl<'a> Ctx<'a> {
    pub(super) fn walk_expr(&mut self, eid: ExprId, binds: &Binds) {
        let exprs: AstExprsView<'a> = self.exprs;
        match &exprs[eid.0 as usize] {
            Expr::Closure { fn_name, captures } => {
                // A FUNCTION EXPRESSION's `this` is its own receiver,
                // never the enclosing scope's: whichever receiver rule
                // later claims the body strips the `__this` capture
                // and hands it an own `__this: any` param (a slot the
                // spec fills per call). Snapshotting the enclosing
                // `__this` ann here pinned the ENCLOSING receiver's
                // class on the callee's own `this` — a nested
                // `function` in a class method sniffed `return this`
                // as the class, and the plain call's `undefined` then
                // flowed through a C-typed slot (store/drop SIGSEGV).
                // An ARROW keeps the entry: §8.3.4 says its `this` IS
                // the enclosing one, and the capture survives to the
                // checker with exactly this ann.
                let own_this = self.fn_expr_exprs.contains(&eid);
                let snap: Binds = captures
                    .iter()
                    .filter(|c| !(own_this && c.as_str() == "__this"))
                    .filter_map(|c| binds.get(c).map(|a| (c.clone(), a.clone())))
                    .collect();
                // First construction site wins (one placeholder per
                // lifted arrow; guards a hypothetical duplicate).
                self.snapshots
                    .closures
                    .entry(fn_name.clone())
                    .or_insert(snap);
            }
            Expr::BinOp { left, right, .. }
            | Expr::Sequence { left, right }
            | Expr::Nullish {
                lhs: left,
                rhs: right,
            } => {
                self.walk_expr(*left, binds);
                self.walk_expr(*right, binds);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Delete { expr }
            | Expr::Spread { expr }
            | Expr::As { expr, .. } => self.walk_expr(*expr, binds),
            Expr::InstanceOf { expr, rhs } => {
                self.walk_expr(*expr, binds);
                self.walk_expr(*rhs, binds);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => self.walk_expr(*obj, binds),
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                self.walk_expr(*obj, binds);
                self.walk_expr(*index, binds);
            }
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                self.walk_expr(*callee, binds);
                for arg in args {
                    self.walk_expr(*arg, binds);
                }
            }
            Expr::Assign { target, value } => {
                self.walk_expr(*target, binds);
                self.walk_expr(*value, binds);
            }
            Expr::Array(elems) => {
                for e in elems {
                    self.walk_expr(*e, binds);
                }
            }
            Expr::ObjectLit { fields } => {
                // A method-carrying literal gets a full binds snapshot:
                // `objlit_nominal` sniffs its data-field anns to mint
                // the `__ObjLit_n` layout and must resolve field-value
                // idents in THIS scope, not the by-name fallback.
                if fields
                    .iter()
                    .any(|(_, e)| self.objlit_method_exprs.contains(e))
                {
                    self.snapshots
                        .objlits
                        .entry(eid.0)
                        .or_insert_with(|| binds.clone());
                }
                for (_, e) in fields {
                    self.walk_expr(*e, binds);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(*cond, binds);
                self.walk_expr(*then_branch, binds);
                self.walk_expr(*else_branch, binds);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    self.walk_expr(*a, binds);
                }
            }
            Expr::PostIncr { target, .. } => self.walk_expr(*target, binds),
            // Pre-lift arrows don't survive to this pass; if one does,
            // its body is its own scope — skip (construction sites
            // inside will take the outer_binds fallback).
            _ => {}
        }
    }
}
