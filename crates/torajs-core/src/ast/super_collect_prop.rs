//! SuperProperty site collection for the class desugar.
//!
//! The parser encodes `super.x` / `super[k]` as a Member / Index off
//! the marker ident `__superbase__` (see `parser/primary_new_super`).
//! This walk surfaces every such site in a method body together with
//! its syntactic role, so `desugar_classes_super_prop` can pick the
//! rewrite per §13.3.7 / §9.1.9:
//!
//!   - `Read`        — `super.x` in read position (getter dispatch or
//!                     a plain read off the super base)
//!   - `IndexRead`   — `super[k]` in read position
//!   - `AssignName`  — `super.x = v` (setter dispatch or a write to
//!                     the `this` receiver); `stmt_pos` records whether
//!                     the Assign is a whole expression statement (the
//!                     only place a Void setter call can replace it)
//!   - `AssignIndex` — `super[k] = v` (write to the `this` receiver)
//!
//! `super[k](...)` / `super.x?.()` call forms are deliberately NOT
//! collected: the callee read would resolve the receiver to the super
//! base instead of `this` (§13.3.6 wants `this`), so their marker is
//! left in place and fails loud at the checker rather than running
//! with the wrong receiver.
//!
//! Every collected marker ExprId is also recorded so the rewrite can
//! overwrite the (then-orphaned) marker nodes with an inert leaf —
//! same posture as the eval desugar's consumed-call overwrite
//! (rotation 333 blade 7): later arena-scanning passes must not trip
//! over an orphan `__superbase__`.

use super::{Ast, Expr, ExprId, Stmt};

pub(super) const SUPER_BASE_MARKER: &str = "__superbase__";

pub(super) enum SuperPropSite {
    Read {
        member: ExprId,
        name: String,
    },
    IndexRead {
        index_expr: ExprId,
    },
    AssignName {
        assign: ExprId,
        target: ExprId,
        name: String,
        value: ExprId,
        stmt_pos: bool,
    },
    AssignIndex {
        target_index: ExprId,
    },
}

#[derive(Default)]
pub(super) struct SuperPropSites {
    pub sites: Vec<SuperPropSite>,
    pub markers: Vec<ExprId>,
}

fn is_marker(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Ident(n) if n == SUPER_BASE_MARKER)
}

pub(super) fn collect_superprop_in_stmt(ast: &Ast, s: &Stmt, out: &mut SuperPropSites) {
    match s {
        // An expression statement whose whole expression is the Assign
        // is the one position where a Void setter call can stand in
        // for the assignment's value.
        Stmt::Expr(eid) => walk_expr(ast, *eid, out, true),
        Stmt::Throw(eid) | Stmt::Yield(eid) => walk_expr(ast, *eid, out, false),
        Stmt::YieldInto { value, .. } => walk_expr(ast, *value, out, false),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                walk_expr(ast, *eid, out, false);
            }
        }
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            walk_expr(ast, *init, out, false)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, out, false);
            collect_superprop_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_superprop_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            walk_expr(ast, *cond, out, false);
            collect_superprop_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_superprop_in_stmt(ast, body, out);
            walk_expr(ast, *cond, out, false);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            walk_expr(ast, *scrutinee, out, false);
            for c in cases {
                walk_expr(ast, c.value, out, false);
                for s in &c.body {
                    collect_superprop_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    collect_superprop_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_superprop_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                walk_expr(ast, *c, out, false);
            }
            if let Some(st) = step {
                walk_expr(ast, *st, out, false);
            }
            collect_superprop_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                collect_superprop_in_stmt(ast, st, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                collect_superprop_in_stmt(ast, st, out);
            }
            for st in catch_body {
                collect_superprop_in_stmt(ast, st, out);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    collect_superprop_in_stmt(ast, st, out);
                }
            }
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Labeled { body, .. } => collect_superprop_in_stmt(ast, body, out),
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            walk_expr(ast, *parent, out, false);
            walk_expr(ast, *sep, out, false);
            collect_superprop_in_stmt(ast, body, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            walk_expr(ast, *elem_expr, out, false);
            collect_superprop_in_stmt(ast, body, out);
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                collect_superprop_in_stmt(ast, inner, out);
            }
        }
    }
}

fn walk_expr(ast: &Ast, eid: ExprId, out: &mut SuperPropSites, stmt_pos: bool) {
    match ast.get_expr(eid) {
        Expr::Member { obj, name } if is_marker(ast, *obj) => {
            out.markers.push(*obj);
            out.sites.push(SuperPropSite::Read {
                member: eid,
                name: name.clone(),
            });
        }
        Expr::Index { obj, index } if is_marker(ast, *obj) => {
            out.markers.push(*obj);
            out.sites.push(SuperPropSite::IndexRead { index_expr: eid });
            walk_expr(ast, *index, out, false);
        }
        Expr::Assign { target, value } => {
            let (target, value) = (*target, *value);
            match ast.get_expr(target) {
                Expr::Member { obj, name } if is_marker(ast, *obj) => {
                    out.markers.push(*obj);
                    out.sites.push(SuperPropSite::AssignName {
                        assign: eid,
                        target,
                        name: name.clone(),
                        value,
                        stmt_pos,
                    });
                }
                Expr::Index { obj, index } if is_marker(ast, *obj) => {
                    out.markers.push(*obj);
                    out.sites.push(SuperPropSite::AssignIndex {
                        target_index: target,
                    });
                    walk_expr(ast, *index, out, false);
                }
                _ => walk_expr(ast, target, out, false),
            }
            walk_expr(ast, value, out, false);
        }
        // `super[k](...)` — receiver must be `this`, which a plain
        // rewrite of the callee read cannot deliver. Skip the callee
        // (marker stays put → loud unknown-ident at the checker),
        // still walk the args.
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            let skip_callee = matches!(
                ast.get_expr(*callee),
                Expr::Index { obj, .. } | Expr::Member { obj, .. } if is_marker(ast, *obj)
            );
            if !skip_callee {
                walk_expr(ast, *callee, out, false);
            }
            for a in args {
                walk_expr(ast, *a, out, false);
            }
        }
        // `super.x++` — read from the base but write to `this`, fused
        // in one node. Not representable by rewriting the single
        // Member; leave the marker for a loud checker error.
        Expr::PostIncr { target, .. } => {
            if !matches!(
                ast.get_expr(*target),
                Expr::Member { obj, .. } | Expr::Index { obj, .. } if is_marker(ast, *obj)
            ) {
                walk_expr(ast, *target, out, false);
            }
        }
        Expr::BinOp { left, right, .. } => {
            walk_expr(ast, *left, out, false);
            walk_expr(ast, *right, out, false);
        }
        Expr::Unary { expr, .. } => walk_expr(ast, *expr, out, false),
        Expr::Member { obj, .. } => walk_expr(ast, *obj, out, false),
        Expr::Index { obj, index } => {
            walk_expr(ast, *obj, out, false);
            walk_expr(ast, *index, out, false);
        }
        Expr::Array(elems) => {
            for e in elems {
                walk_expr(ast, *e, out, false);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, out, false);
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_superprop_in_stmt(ast, s, out);
            }
        }
        Expr::Closure { .. } => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                walk_expr(ast, *a, out, false);
            }
        }
        Expr::NewDynamic { callee, args } => {
            walk_expr(ast, *callee, out, false);
            for a in args {
                walk_expr(ast, *a, out, false);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, out, false);
            walk_expr(ast, *then_branch, out, false);
            walk_expr(ast, *else_branch, out, false);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => walk_expr(ast, *expr, out, false),
        Expr::InstanceOf { expr, rhs } => {
            walk_expr(ast, *expr, out, false);
            walk_expr(ast, *rhs, out, false);
        }
        Expr::Sequence { left, right } => {
            walk_expr(ast, *left, out, false);
            walk_expr(ast, *right, out, false);
        }
        Expr::Nullish { lhs, rhs } => {
            walk_expr(ast, *lhs, out, false);
            walk_expr(ast, *rhs, out, false);
        }
        Expr::OptChain { obj, .. } => walk_expr(ast, *obj, out, false),
        Expr::OptIndex { obj, index } => {
            walk_expr(ast, *obj, out, false);
            walk_expr(ast, *index, out, false);
        }
        _ => {}
    }
}
