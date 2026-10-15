//! Does a node write an owned value INTO the binding `x`, or hand
//! `x` on as a bare value — the predicate [`crate::ast::let_owned_elems`]
//! asks of every statement in a binding's window. The rule and its
//! bias (a shape the walk cannot prove safe counts as a write) are
//! documented there.

use crate::ast::let_owned_elems_alias::{AliasNames, alias_edge};
use crate::ast::let_owned_elems_nest::{self as nest, Nest};
use crate::ast::view_homes::stores_owned;
use crate::ast::{Ast, BinOp, Expr, ExprId, Stmt};

/// The binding asked about, and whether a bare alias between two
/// connectable bindings (`y = x` / `let y = x`,
/// [`crate::ast::let_owned_elems_alias`]) counts as a write. With `None`
/// it does, which is the verdict a poisoned alias class falls back
/// to. `nest` is set when `x` is a container's element class
/// ([`crate::ast::let_owned_elems_nest`]).
pub(super) struct Query<'a> {
    pub x: &'a str,
    pub aliases: Option<&'a AliasNames>,
    pub nest: Option<&'a Nest<'a>>,
}

impl Query<'_> {
    fn alias(&self, ast: &Ast, target: &str, value: ExprId) -> bool {
        self.aliases
            .is_some_and(|names| alias_edge(ast, names, target, value).is_some())
    }

    /// A store of a bare value along an alias edge: `t = v`, `C[e] = v`,
    /// `C.push(v)`.
    fn alias_store(&self, ast: &Ast, e: ExprId) -> bool {
        self.aliases.is_some_and(|names| {
            nest::edge_slot(ast, names, ast.get_expr(e))
                .is_some_and(|(t, v)| alias_edge(ast, names, t, v).is_some())
        })
    }
}

/// `x` itself, through the `as` wrappers a use site can carry and
/// the forms that answer one of their operands unchanged.
fn is_bare(ast: &Ast, eid: ExprId, x: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == x,
        Expr::As { expr, .. } => is_bare(ast, *expr, x),
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => is_bare(ast, *then_branch, x) || is_bare(ast, *else_branch, x),
        Expr::Nullish { lhs, rhs } => is_bare(ast, *lhs, x) || is_bare(ast, *rhs, x),
        Expr::BinOp {
            op: BinOp::LAnd | BinOp::LOr,
            left,
            right,
        } => is_bare(ast, *left, x) || is_bare(ast, *right, x),
        Expr::Sequence { right, .. } => is_bare(ast, *right, x),
        _ => false,
    }
}

// The walk below is the owned-elements twin of
// `ssa_lower_toplevel_globals::write_through::binding_written_through`
// (same variant catalogue, a different question): true when `x` is
// written INTO with an owned value, or escapes as a bare value,
// somewhere under the node.
fn expr_writes(ast: &Ast, eid: ExprId, q: &Query<'_>) -> bool {
    let x = q.x;
    if let Some(n) = q.nest
        && let Some(v) = nest::container_use(ast, eid, n, &|t, v| q.alias(ast, t, v), &|e| {
            expr_writes(ast, e, q)
        })
    {
        return v;
    }
    let is_x = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(n) if n == x);
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            if let Expr::Member { obj, name } = ast.get_expr(*callee)
                && is_x(*obj)
                && stores_owned(name)
            {
                return true;
            }
            // the whole array handed to a callee: an escape, unless the
            // callee is a container's push along an alias edge
            if args.iter().any(|a| is_bare(ast, *a, x)) && !q.alias_store(ast, eid) {
                return true;
            }
            expr_writes(ast, *callee, q) || args.iter().any(|a| expr_writes(ast, *a, q))
        }
        Expr::Assign { target, value } => {
            // an element or a property of the binding written
            // (`x[i] = v`, `x.length = n`): a length store resizes the
            // block — grown slots are holes, which a view-typed read
            // has no undefined for — so, like the global lane, every
            // write through the binding keeps its elements owned
            let target_is_through_x = match ast.get_expr(*target) {
                Expr::Index { obj, .. } | Expr::Member { obj, .. } => is_x(*obj),
                _ => false,
            };
            // the binding itself reassigned (`x = [...]` / `x = other`):
            // its slot has to take whatever the program stores later,
            // and an `Arr<Substr>` slot takes only a fresh split
            // product — so it is typed `Arr<Str>` from the start and
            // every value entering it is owned (plan-state 468-02) —
            // unless the two sides are an alias class that agrees on
            // the layout. The same edge, seen from the value side, is
            // then no escape either.
            let rebound = is_x(*target);
            let alias = q.alias_store(ast, eid);
            // the whole array stored anywhere: an escape
            target_is_through_x
                || ((rebound || is_bare(ast, *value, x)) && !alias)
                || expr_writes(ast, *target, q)
                || expr_writes(ast, *value, q)
        }
        Expr::PostIncr { target, .. } => {
            matches!(ast.get_expr(*target), Expr::Index { obj, .. } | Expr::Member { obj, .. } if is_x(*obj))
                || expr_writes(ast, *target, q)
        }
        // Anything that can hide a write we have not modelled
        // counts: a lifted closure that captured the binding, or
        // an arrow whose body is not walked here.
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == x),
        Expr::ArrowFn { .. } => true,
        // Pure recursion over every other variant — the same
        // catalogue `ast::escape_analyze::eal_expr_safe` walks.
        Expr::Member { obj, .. } => expr_writes(ast, *obj, q),
        Expr::Index { obj, index } => expr_writes(ast, *obj, q) || expr_writes(ast, *index, q),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            expr_writes(ast, *left, q) || expr_writes(ast, *right, q)
        }
        Expr::Nullish { lhs, rhs } => expr_writes(ast, *lhs, q) || expr_writes(ast, *rhs, q),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::As { expr, .. }
        | Expr::Spread { expr } => expr_writes(ast, *expr, q),
        Expr::InstanceOf { expr, rhs } => expr_writes(ast, *expr, q) || expr_writes(ast, *rhs, q),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, q)
                || expr_writes(ast, *then_branch, q)
                || expr_writes(ast, *else_branch, q)
        }
        Expr::Array(els) => els
            .iter()
            .any(|e| is_bare(ast, *e, x) || expr_writes(ast, *e, q)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| is_bare(ast, *e, x) || expr_writes(ast, *e, q)),
        Expr::OptChain { obj, .. } => expr_writes(ast, *obj, q),
        Expr::OptIndex { obj, index } => expr_writes(ast, *obj, q) || expr_writes(ast, *index, q),
        Expr::OptCall { callee, args } => {
            expr_writes(ast, *callee, q) || args.iter().any(|a| expr_writes(ast, *a, q))
        }
        Expr::New { args, .. } | Expr::Super { args } => args
            .iter()
            .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, q)),
        Expr::NewDynamic { callee, args } => {
            expr_writes(ast, *callee, q)
                || args
                    .iter()
                    .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, q))
        }
        Expr::Ident(_)
        | Expr::Elision
        | Expr::This
        | Expr::NewTarget
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => false,
    }
}
pub(super) fn stmt_writes(ast: &Ast, s: &Stmt, q: &Query<'_>) -> bool {
    let x = q.x;
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => expr_writes(ast, *e, q),
        // the whole array yielded, returned, or bound again: an escape
        // — the receiver is typed by its own annotation
        Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
            is_bare(ast, *e, x) || expr_writes(ast, *e, q)
        }
        Stmt::Return(e) => e.is_some_and(|e| is_bare(ast, e, x) || expr_writes(ast, e, q)),
        Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::LetDecl { name, init, .. } => {
            if let Some(n) = q.nest
                && let Some(v) =
                    nest::let_use(ast, name, *init, n, &|t, v| q.alias(ast, t, v), &|e| {
                        expr_writes(ast, e, q)
                    })
            {
                return v;
            }
            (is_bare(ast, *init, x) && !q.alias(ast, name, *init)) || expr_writes(ast, *init, q)
        }
        Stmt::UsingDecl { init, .. } => is_bare(ast, *init, x) || expr_writes(ast, *init, q),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, q)
                || stmt_writes(ast, then_branch, q)
                || else_branch
                    .as_deref()
                    .is_some_and(|e| stmt_writes(ast, e, q))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_writes(ast, *cond, q) || stmt_writes(ast, body, q)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref().is_some_and(|i| stmt_writes(ast, i, q))
                || cond.is_some_and(|c| expr_writes(ast, c, q))
                || step.is_some_and(|st| expr_writes(ast, st, q))
                || stmt_writes(ast, body, q)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_writes(ast, *parent, q) || expr_writes(ast, *sep, q) || stmt_writes(ast, body, q),
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_writes(ast, *elem_expr, q) || stmt_writes(ast, body, q),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            expr_writes(ast, *scrutinee, q)
                || cases.iter().any(|c| {
                    expr_writes(ast, c.value, q) || c.body.iter().any(|s| stmt_writes(ast, s, q))
                })
                || default
                    .as_ref()
                    .is_some_and(|db| db.iter().any(|s| stmt_writes(ast, s, q)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_writes(ast, s, q))
                || catch_body.iter().any(|s| stmt_writes(ast, s, q))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_writes(ast, s, q)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_writes(ast, s, q)),
        Stmt::Labeled { body, .. } => stmt_writes(ast, body, q),
        // A named fn body CAN reach a promoted top-level binding
        // (that is why it was promoted); walk it. Class methods
        // likewise.
        Stmt::FnDecl { body, .. } => body.iter().any(|s| stmt_writes(ast, s, q)),
        Stmt::ClassDecl { methods, .. } => methods
            .iter()
            .any(|m| m.body.iter().any(|s| stmt_writes(ast, s, q))),
        Stmt::ExportDecl { inner, .. } => inner.as_deref().is_some_and(|s| stmt_writes(ast, s, q)),
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => false,
    }
}
