//! Which `let` / `const` / `var` bindings must hold OWNED elements —
//! the census the split-product binding lane consults (rotation 468,
//! the write face of plan-state 467-01 and the escape face found
//! alongside it).
//!
//! `s.split(sep)` answers an array of substring VIEWS: 32-byte cells
//! in the split block's tail that read their text through the parent
//! string, typed `Arr<Substr>`. That representation is only right
//! while the product stays in its typed-tier home — read by index,
//! measured, iterated, joined. Two things break it:
//!
//! - an owned WRITE into it — `X.push(v)` / `unshift` / `splice` /
//!   `fill` / `copyWithin`, or `X[i] = v` — because an `Arr<Substr>`
//!   slot cannot take an owned string and every reader decodes the
//!   slots by the view layout (`a.push("z"+"w"); a.join("+")`
//!   SIGSEGV'd);
//! - an ESCAPE of the whole array as a bare value — a call or `new`
//!   argument, the init of another binding, the value of an
//!   assignment, an array or object literal element, a return or
//!   yield — because the receiving side is typed by its own
//!   annotation (`string[]` → `Arr<Str>`, or `any`) and decodes the
//!   views as owned strings (`addx(parts)` with `xs: string[]` printed
//!   garbage; `let b = a; b.push(..)` crashed; `let v: any = a` then
//!   `for (const x of v)` read freed memory).
//!
//! For a binding listed here, the lane materializes the fresh product
//! in place (every view becomes an owned string — torajs-arr
//! `substr_materialize.rs`) and types the binding `Arr<Str>`, where
//! the mutators store any string shape and every receiver's
//! annotation already agrees. Sort / reverse / pop / shift only
//! permute or remove and are not writes; element reads, `.length`,
//! for-of, join and the other in-home consumers are not escapes.
//!
//! ## Shape
//!
//! Sibling of [`super::regex_result_props`] and
//! [`super::escape_analyze`]: the same per-binding window (a `let` is
//! in scope from its declaration to the end of its block, so the
//! trailing statements are every place it can be used — plus the
//! block's hoisted function declarations; a top-level binding is
//! reachable from every function body, so its window is the whole
//! program) and the same bias rule: a shape the walk cannot prove is
//! not a write or an escape COUNTS as one (a lifted closure that
//! captured the binding, an arrow whose body is not walked). A false
//! positive only materializes a product that did not need it; a false
//! negative stores an owned cell into a view-typed array or hands
//! views to a reader that cannot tell. Keyed by the declaration's
//! init ExprId, which is what the binding lane holds when it decides.

use std::collections::HashSet;

use super::{Ast, Expr, ExprId, Stmt};

/// Record the init ExprIds of the bindings that must hold owned
/// elements: written into with an owned value, or escaping as a bare
/// value, somewhere in their scope. Runs at the end of the desugar
/// pipeline for the siblings' reason: the census has to see the final
/// shape of every use of the binding.
pub fn analyze_let_owned_elems(ast: &mut Ast) {
    let mut found: HashSet<ExprId> = HashSet::new();
    let stmts = ast.stmts.clone();
    low_walk_stmts(ast, &stmts, true, &mut found);
    ast.let_owned_elem_inits = found;
}

fn low_walk_stmts(ast: &Ast, stmts: &[Stmt], top_level: bool, found: &mut HashSet<ExprId>) {
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::LetDecl { name, init, .. } = s {
            // The trailing statements are every place the binding
            // can be written — and a hoisted fn decl anywhere in the
            // block can reach it too. At top level every function
            // body can reach the binding, so the whole program is the
            // window.
            let written = if top_level {
                stmts.iter().any(|t| stmt_writes(ast, t, name))
            } else {
                stmts[i + 1..].iter().any(|t| stmt_writes(ast, t, name))
                    || stmts
                        .iter()
                        .any(|t| matches!(t, Stmt::FnDecl { .. }) && stmt_writes(ast, t, name))
            };
            if written {
                found.insert(*init);
            }
        }
    }
    for s in stmts {
        low_recurse_into(ast, s, found);
    }
}

fn low_recurse_into(ast: &Ast, s: &Stmt, found: &mut HashSet<ExprId>) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => low_walk_stmts(ast, inner, false, found),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            low_recurse_into(ast, then_branch, found);
            if let Some(eb) = else_branch {
                low_recurse_into(ast, eb, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => low_recurse_into(ast, body, found),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                low_recurse_into(ast, i, found);
            }
            low_recurse_into(ast, body, found);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            low_recurse_into(ast, body, found)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                low_walk_stmts(ast, &c.body, false, found);
            }
            if let Some(db) = default {
                low_walk_stmts(ast, db, false, found);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            low_walk_stmts(ast, body, false, found);
            low_walk_stmts(ast, catch_body, false, found);
            if let Some(fb) = finally_body {
                low_walk_stmts(ast, fb, false, found);
            }
        }
        Stmt::FnDecl { body, .. } => low_walk_stmts(ast, body, false, found),
        Stmt::ClassDecl { methods, .. } => {
            for m in methods {
                low_walk_stmts(ast, &m.body, false, found);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                low_recurse_into(ast, inner, found);
            }
        }
        Stmt::Labeled { body, .. } => low_recurse_into(ast, body, found),
        _ => {}
    }
}

/// `x` itself, through the `as` wrappers a use site can carry.
fn is_bare(ast: &Ast, eid: ExprId, x: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n == x,
        Expr::As { expr, .. } => is_bare(ast, *expr, x),
        _ => false,
    }
}

// The walk below is the owned-elements twin of
// `ssa_lower_toplevel_globals::write_through::binding_written_through`
// (same variant catalogue, a different question): true when `x` is
// written INTO with an owned value, or escapes as a bare value,
// somewhere under the node.
fn expr_writes(ast: &Ast, eid: ExprId, x: &str) -> bool {
    let is_x = |e: ExprId| matches!(ast.get_expr(e), Expr::Ident(n) if n == x);
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            if let Expr::Member { obj, name } = ast.get_expr(*callee)
                && is_x(*obj)
                && matches!(
                    name.as_str(),
                    "push" | "unshift" | "splice" | "fill" | "copyWithin"
                )
            {
                return true;
            }
            // the whole array handed to a callee: an escape
            if args.iter().any(|a| is_bare(ast, *a, x)) {
                return true;
            }
            expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
        }
        Expr::Assign { target, value } => {
            let target_is_through_x =
                matches!(ast.get_expr(*target), Expr::Index { obj, .. } if is_x(*obj));
            // the whole array stored anywhere: an escape
            target_is_through_x
                || is_bare(ast, *value, x)
                || expr_writes(ast, *target, x)
                || expr_writes(ast, *value, x)
        }
        Expr::PostIncr { target, .. } => {
            matches!(ast.get_expr(*target), Expr::Index { obj, .. } if is_x(*obj))
                || expr_writes(ast, *target, x)
        }
        // Anything that can hide a write we have not modelled
        // counts: a lifted closure that captured the binding, or
        // an arrow whose body is not walked here.
        Expr::Closure { captures, .. } => captures.iter().any(|c| c == x),
        Expr::ArrowFn { .. } => true,
        // Pure recursion over every other variant — the same
        // catalogue `ast::escape_analyze::eal_expr_safe` walks.
        Expr::Member { obj, .. } => expr_writes(ast, *obj, x),
        Expr::Index { obj, index } => expr_writes(ast, *obj, x) || expr_writes(ast, *index, x),
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            expr_writes(ast, *left, x) || expr_writes(ast, *right, x)
        }
        Expr::Nullish { lhs, rhs } => expr_writes(ast, *lhs, x) || expr_writes(ast, *rhs, x),
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::As { expr, .. }
        | Expr::Spread { expr } => expr_writes(ast, *expr, x),
        Expr::InstanceOf { expr, rhs } => expr_writes(ast, *expr, x) || expr_writes(ast, *rhs, x),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, x)
                || expr_writes(ast, *then_branch, x)
                || expr_writes(ast, *else_branch, x)
        }
        Expr::Array(els) => els
            .iter()
            .any(|e| is_bare(ast, *e, x) || expr_writes(ast, *e, x)),
        Expr::ObjectLit { fields } => fields
            .iter()
            .any(|(_, e)| is_bare(ast, *e, x) || expr_writes(ast, *e, x)),
        Expr::OptChain { obj, .. } => expr_writes(ast, *obj, x),
        Expr::OptIndex { obj, index } => expr_writes(ast, *obj, x) || expr_writes(ast, *index, x),
        Expr::OptCall { callee, args } => {
            expr_writes(ast, *callee, x) || args.iter().any(|a| expr_writes(ast, *a, x))
        }
        Expr::New { args, .. } | Expr::Super { args } => args
            .iter()
            .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, x)),
        Expr::NewDynamic { callee, args } => {
            expr_writes(ast, *callee, x)
                || args
                    .iter()
                    .any(|a| is_bare(ast, *a, x) || expr_writes(ast, *a, x))
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
fn stmt_writes(ast: &Ast, s: &Stmt, x: &str) -> bool {
    match s {
        Stmt::Expr(e) | Stmt::Throw(e) => expr_writes(ast, *e, x),
        // the whole array yielded, returned, or bound again: an escape
        // — the receiver is typed by its own annotation
        Stmt::Yield(e) | Stmt::YieldInto { value: e, .. } => {
            is_bare(ast, *e, x) || expr_writes(ast, *e, x)
        }
        Stmt::Return(e) => e.is_some_and(|e| is_bare(ast, e, x) || expr_writes(ast, e, x)),
        Stmt::Break(_) | Stmt::Continue(_) => false,
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            is_bare(ast, *init, x) || expr_writes(ast, *init, x)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_writes(ast, *cond, x)
                || stmt_writes(ast, then_branch, x)
                || else_branch
                    .as_deref()
                    .is_some_and(|e| stmt_writes(ast, e, x))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            expr_writes(ast, *cond, x) || stmt_writes(ast, body, x)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref().is_some_and(|i| stmt_writes(ast, i, x))
                || cond.is_some_and(|c| expr_writes(ast, c, x))
                || step.is_some_and(|st| expr_writes(ast, st, x))
                || stmt_writes(ast, body, x)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => expr_writes(ast, *parent, x) || expr_writes(ast, *sep, x) || stmt_writes(ast, body, x),
        Stmt::ForOf {
            elem_expr, body, ..
        } => expr_writes(ast, *elem_expr, x) || stmt_writes(ast, body, x),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            expr_writes(ast, *scrutinee, x)
                || cases.iter().any(|c| {
                    expr_writes(ast, c.value, x) || c.body.iter().any(|s| stmt_writes(ast, s, x))
                })
                || default
                    .as_ref()
                    .is_some_and(|db| db.iter().any(|s| stmt_writes(ast, s, x)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_writes(ast, s, x))
                || catch_body.iter().any(|s| stmt_writes(ast, s, x))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_writes(ast, s, x)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(|s| stmt_writes(ast, s, x)),
        Stmt::Labeled { body, .. } => stmt_writes(ast, body, x),
        // A named fn body CAN reach a promoted top-level binding
        // (that is why it was promoted); walk it. Class methods
        // likewise.
        Stmt::FnDecl { body, .. } => body.iter().any(|s| stmt_writes(ast, s, x)),
        Stmt::ClassDecl { methods, .. } => methods
            .iter()
            .any(|m| m.body.iter().any(|s| stmt_writes(ast, s, x))),
        Stmt::ExportDecl { inner, .. } => inner.as_deref().is_some_and(|s| stmt_writes(ast, s, x)),
        Stmt::TypeDecl { .. } | Stmt::ImportDecl { .. } => false,
    }
}
