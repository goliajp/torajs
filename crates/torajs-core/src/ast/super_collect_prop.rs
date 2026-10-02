//! SuperProperty site collection for the class desugar.
//!
//! The parser encodes `super.x` / `super[k]` as a Member / Index off
//! the marker ident `__superbase__` (see `parser/primary_new_super`).
//! This walk surfaces every such site in a method body together with
//! its syntactic role, so `desugar_classes_super_prop` can pick the
//! rewrite per §13.3.7 / §9.1.9:
//!
//!   - `Read`        — `super.x` in read position (getter dispatch or
//!     a plain read off the super base)
//!   - `IndexRead`   — `super[k]` in read position
//!   - `AssignName`  — `super.x = v` (setter dispatch or a write to
//!     the `this` receiver); `stmt_pos` records whether
//!     the Assign is a whole expression statement (the
//!     only place a Void setter call can replace it)
//!   - `AssignIndex` — `super[k] = v` (write to the `this` receiver)
//!
//!   - `CallIndex`   — `super[k](args)`, whose callee is NOT a
//!     readable value: §13.3.6 invokes the Super
//!     Reference with the CURRENT `this` as receiver,
//!     so the site travels whole to a kernel that
//!     gets-then-calls rather than being split
//!     (rewriting just the callee read would hand the
//!     super base to the callee — right answer for
//!     `super.toString()`, silently wrong for
//!     anything that reads `this`)
//!
//! `super.x?.()` optional-call forms are still NOT collected: the
//! nullish short-circuit is not representable by rewriting the call
//! node alone, so their marker is left in place and fails loud at the
//! checker rather than running with the wrong receiver.
//!
//! Every collected marker ExprId is also recorded so the rewrite can
//! overwrite the (then-orphaned) marker nodes with an inert leaf —
//! same posture as the eval desugar's consumed-call overwrite
//! (rotation 333 blade 7): later arena-scanning passes must not trip
//! over an orphan `__superbase__`.

use super::{Ast, Expr, ExprId, Stmt};

pub(super) const SUPER_BASE_MARKER: &str = "__superbase__";

/// Every expression standing as the operand of a `delete`.
///
/// §13.5.1.2 step 5b makes `delete super.x` a ReferenceError, which
/// neither rewrite spells; both keep the pre-kernel plain member /
/// index off the base there, the approximation this family has always
/// carried. Routing a delete operand through the read kernel would
/// not fix that and WOULD break it further — `delete <a call>` is not
/// a property reference at all, so the checker rejects the whole
/// program (measured: `delete super[(super(), 0)]` went from pass to
/// a compile reject).
pub(super) fn delete_operands(ast: &Ast) -> std::collections::HashSet<ExprId> {
    ast.exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Delete { expr } => Some(*expr),
            _ => None,
        })
        .collect()
}

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
        assign: ExprId,
        target_index: ExprId,
    },
    /// `super.m?.(…)` / `super[k]?.(…)` — §13.3.9.1 OptionalChain over
    /// the reference: the same whole-site shape as `CallIndex` (the
    /// receiver is the call site's `this`), with the nullish
    /// short-circuit inside the kernel. Both key spellings travel
    /// here — the name form has no `__supercall__` marker under `?.`.
    CallOpt {
        call: ExprId,
        /// The Member / Index the marker hangs off (orphaned after).
        callee: ExprId,
    },
    /// `super.x++` / `super[k]++` and the prefix / `--` spellings —
    /// the whole UpdateExpression travels as one site: the read is
    /// off the base with `this` as receiver and the write lands on
    /// `this`, fused in one node that must answer the old value
    /// (postfix) or the new one (prefix) — a kernel's job, not a
    /// rewrite of the Member alone.
    Update {
        node: ExprId,
        /// The `super.x` / `super[k]` Member or Index the marker hangs
        /// off — orphaned once the node becomes the kernel call.
        target: ExprId,
        is_inc: bool,
        is_prefix: bool,
    },
    CallIndex {
        call: ExprId,
        index_expr: ExprId,
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

/// An expression position with no statement around it — a static
/// field's initializer (`static f = super.x`), whose `super` sites
/// belong to the class's static context (§15.7.14 step 31 evaluates
/// it with the class as `this` and the class as the home object).
pub(super) fn collect_superprop_in_expr(ast: &Ast, eid: ExprId, out: &mut SuperPropSites) {
    walk_expr(ast, eid, out, false);
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
            ..
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
                        assign: eid,
                        target_index: target,
                    });
                    walk_expr(ast, *index, out, false);
                }
                _ => walk_expr(ast, target, out, false),
            }
            walk_expr(ast, value, out, false);
        }
        // `super[k](...)` — the receiver must be `this`, which a
        // rewrite of the callee read alone cannot deliver, so the
        // whole call travels as one site. `super.m(...)` is already
        // claimed upstream by the `__supercall__<m>` marker the
        // parser writes for the name form, and an OptCall's nullish
        // short-circuit has no such whole-site shape yet — both leave
        // the callee alone (marker stays put → loud unknown-ident at
        // the checker). Args are walked either way.
        Expr::Call { callee, args } => {
            match ast.get_expr(*callee) {
                Expr::Index { obj, .. } if is_marker(ast, *obj) => {
                    out.markers.push(*obj);
                    out.sites.push(SuperPropSite::CallIndex {
                        call: eid,
                        index_expr: *callee,
                    });
                    let Expr::Index { index, .. } = ast.get_expr(*callee) else {
                        unreachable!()
                    };
                    walk_expr(ast, *index, out, false);
                }
                Expr::Member { obj, .. } if is_marker(ast, *obj) => {}
                _ => walk_expr(ast, *callee, out, false),
            }
            for a in args {
                walk_expr(ast, *a, out, false);
            }
        }
        // `super.m?.()` / `super[k]?.()` — the whole site, like
        // `CallIndex`, into the optional-call kernel (a computed
        // key's expression is walked for sites of its own).
        Expr::OptCall { callee, args } => {
            match ast.get_expr(*callee) {
                Expr::Index { obj, index } if is_marker(ast, *obj) => {
                    out.markers.push(*obj);
                    out.sites.push(SuperPropSite::CallOpt {
                        call: eid,
                        callee: *callee,
                    });
                    walk_expr(ast, *index, out, false);
                }
                Expr::Member { obj, .. } if is_marker(ast, *obj) => {
                    out.markers.push(*obj);
                    out.sites.push(SuperPropSite::CallOpt {
                        call: eid,
                        callee: *callee,
                    });
                }
                _ => walk_expr(ast, *callee, out, false),
            }
            for a in args {
                walk_expr(ast, *a, out, false);
            }
        }
        // `super.x++` — read from the base but write to `this`, fused
        // in one node: the whole UpdateExpression is the site (the
        // `_update` kernel). A computed key's expression is walked
        // for sites of its own.
        Expr::PostIncr {
            target,
            is_inc,
            is_prefix,
        } => match ast.get_expr(*target) {
            Expr::Member { obj, .. } | Expr::Index { obj, .. } if is_marker(ast, *obj) => {
                out.markers.push(*obj);
                out.sites.push(SuperPropSite::Update {
                    node: eid,
                    target: *target,
                    is_inc: *is_inc,
                    is_prefix: *is_prefix,
                });
                if let Expr::Index { index, .. } = ast.get_expr(*target) {
                    walk_expr(ast, *index, out, false);
                }
            }
            _ => walk_expr(ast, *target, out, false),
        },
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
