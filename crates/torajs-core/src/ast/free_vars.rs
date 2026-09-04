//! Free-variable analysis for arrow-fn bodies.
//!
//! Chunk 346 — extracted from ast.rs. The entry `free_vars_of_arrow`
//! and its sibling-private walkers (recursive arena traversal +
//! `is_global_name` filter for built-in receivers) form one logical
//! unit. Caller is one site in ast.rs (line ~3037, inside the
//! arrow-fn lift pass) that resolves via the pub(super) marker on
//! free_vars_of_arrow.
//!
//! This file is the statement half — `walk_stmt` and the frames it
//! opens. The expression half lives next door in
//! [`super::free_vars_expr`]; the two call each other, because a
//! statement holds expressions and an expression can hold a body.

use super::free_vars_decls::{walk_class_decl, walk_fn_decl};
use super::free_vars_expr::walk_expr;
use super::free_vars_hoisted_names::{hoist_annexb_fn_names, hoist_fn_decl_names, hoist_var_names};
use super::{Ast, Param, Stmt};

/// Free-variable analysis for an arrow fn body. Returns a deterministic,
/// de-duplicated list of identifier names referenced in the body that are
/// NOT bound by the arrow's params and NOT declared by any inner let/for
/// in the body itself. The ordering matches first-use order in the body
/// (deterministic across runs).
///
/// Limitations: this is a conservative name-only analysis — it does not
/// distinguish global FnDecls from outer locals (the lowerer filters
/// global fn names out of the capture set when it has the symbol table).
/// Inner ArrowFn bodies are walked too; their inner-arrow params shadow
/// matching names inside their body.
pub(super) fn free_vars_of_arrow(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    global_fn_names: &[String],
    self_name: Option<&str>,
) -> Vec<String> {
    // Pre-bind top-level fn names so they're treated as already-in-scope
    // and don't fall into the captures set.
    let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    bound.extend(global_fn_names.iter().cloned());
    // `arguments` is every function's own quasi-binding (rewritten by
    // desugar_arguments_object AFTER the lift) — never a capture.
    // Pre-613 it leaked into the captures set and the checker rejected
    // the closure with "unknown identifier `arguments`".
    bound.push("arguments".into());
    // §15.5.5 — a named fn-expression's self-name is the function
    // env's own binding (the mint site writes the cell into a trailing
    // env slot), never something the enclosing scope must supply.
    if let Some(sn) = self_name {
        bound.push(sn.to_string());
    }
    free_vars_of_body(ast, &bound, body)
}

/// The walk itself, over a caller-supplied pre-bound set. Split out of
/// [`free_vars_of_arrow`] so the nested-fn capture router
/// ([`super::nested_fns_capture`]) decides the same question from the
/// same walk instead of growing a second one — two copies of a
/// control-flow-sensitive traversal is exactly the thing that drifts.
/// That caller wants `arguments` LEFT free (its presence is what tells
/// it a declaration cannot become an arrow), so the quasi-binding is
/// the wrapper's business, not the walk's.
pub(super) fn free_vars_of_body(ast: &Ast, prebound: &[String], body: &[Stmt]) -> Vec<String> {
    let mut bound: Vec<String> = prebound.to_vec();
    let mut out: Vec<String> = Vec::new();
    hoist_fn_decl_names(body, &mut bound);
    // `var` is function-scoped, so a declaration written inside a block
    // is bound for the whole body — including after the block, where
    // the block-scoped walk below would have dropped it.
    hoist_var_names(body, &mut bound);
    // Annex B §B.3.3 hoists a block-nested `function` the same way in
    // sloppy code — see `hoist_annexb_fn_names`.
    hoist_annexb_fn_names(ast, body, &mut bound);
    for s in body {
        walk_stmt(ast, s, &mut bound, &mut out);
    }
    out
}

/// 423-01 knife C — the free identifier names of ONE top-level lib
/// statement, for the module resolver's hidden-dependency census.
/// Same walk as the arrow lift with nothing pre-bound: the caller
/// intersects the answer against the lib's own top-level decl names,
/// so over-reporting (builtins, entry spellings) is harmless.
pub(crate) fn free_idents_of_stmt(ast: &Ast, s: &Stmt) -> Vec<String> {
    free_vars_of_body(ast, &[], std::slice::from_ref(s))
}

/// The try / catch / finally scopes: each block is its own binding
/// region, and the catch param binds only inside the catch body.
fn walk_try(
    ast: &Ast,
    body: &[Stmt],
    catch_param: &Option<String>,
    catch_body: &[Stmt],
    finally_body: &Option<Vec<Stmt>>,
    bound: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    let saved = bound.len();
    hoist_fn_decl_names(body, bound);
    for s in body {
        walk_stmt(ast, s, bound, out);
    }
    bound.truncate(saved);
    if let Some(name) = catch_param {
        bound.push(name.clone());
    }
    hoist_fn_decl_names(catch_body, bound);
    for s in catch_body {
        walk_stmt(ast, s, bound, out);
    }
    bound.truncate(saved);
    if let Some(fb) = finally_body {
        hoist_fn_decl_names(fb, bound);
        for s in fb {
            walk_stmt(ast, s, bound, out);
        }
        bound.truncate(saved);
    }
}

pub(super) fn walk_stmt(ast: &Ast, s: &Stmt, bound: &mut Vec<String>, out: &mut Vec<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) => {
            walk_expr(ast, *eid, bound, out)
        }
        // YieldInto is the parse-time spelling of `let var = yield value`
        // (yield_expr_hoist) — the temp BINDS from here on, exactly like
        // LetDecl below. Without the bind every downstream `__yx_N` read
        // reported free, so a generator EXPRESSION whose body used
        // expression-position yield always "captured" its own temp and
        // the hoist pass panicked.
        Stmt::YieldInto { var, value, .. } => {
            walk_expr(ast, *value, bound, out);
            bound.push(var.clone());
        }
        Stmt::Return(None) => {}
        Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
            walk_expr(ast, *init, bound, out);
            bound.push(name.clone());
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, bound, out);
            let saved = bound.len();
            walk_stmt(ast, then_branch, bound, out);
            bound.truncate(saved);
            if let Some(eb) = else_branch {
                walk_stmt(ast, eb, bound, out);
                bound.truncate(saved);
            }
        }
        Stmt::While { cond, body } => {
            walk_expr(ast, *cond, bound, out);
            let saved = bound.len();
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::DoWhile { body, cond } => {
            let saved = bound.len();
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
            walk_expr(ast, *cond, bound, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            walk_expr(ast, *scrutinee, bound, out);
            // §14.12.4 — the CaseBlock is ONE declarative environment
            // spanning every clause and the default, not one per
            // clause: `case 0: let a = 1; case 1: a` reads the same
            // binding. So the frame opens once here and the fn-decl
            // names of every clause hoist into it together.
            let saved = bound.len();
            for c in cases.iter() {
                hoist_fn_decl_names(&c.body, bound);
            }
            if let Some(db) = default {
                hoist_fn_decl_names(db, bound);
            }
            for c in cases {
                walk_expr(ast, c.value, bound, out);
                for s in &c.body {
                    walk_stmt(ast, s, bound, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    walk_stmt(ast, s, bound, out);
                }
            }
            bound.truncate(saved);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let saved = bound.len();
            if let Some(i) = init {
                walk_stmt(ast, i, bound, out);
            }
            if let Some(c) = cond {
                walk_expr(ast, *c, bound, out);
            }
            if let Some(st) = step {
                walk_expr(ast, *st, bound, out);
            }
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::Block(stmts) => {
            let saved = bound.len();
            hoist_fn_decl_names(stmts, bound);
            for st in stmts {
                walk_stmt(ast, st, bound, out);
            }
            bound.truncate(saved);
        }
        Stmt::Multi(stmts) => {
            // Same surrounding scope — bindings stay visible after.
            hoist_fn_decl_names(stmts, bound);
            for st in stmts {
                walk_stmt(ast, st, bound, out);
            }
        }
        Stmt::ForOfSplitIter {
            var_name,
            parent,
            sep,
            body,
        } => {
            // Same scope hygiene as Stmt::For — var_name binds inside
            // the body only.
            walk_expr(ast, *parent, bound, out);
            walk_expr(ast, *sep, bound, out);
            let saved = bound.len();
            bound.push(var_name.clone());
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::ForOf {
            var_name,
            i_ident,
            elem_expr,
            body,
            ..
        } => {
            // The counter is minted by the for-of desugar and lives
            // only inside this loop — but `elem_expr` IS `src[i]`, so
            // walking it before binding the counter reported `i` as a
            // free variable. Only arrows ask this question
            // (`free_vars_of_arrow` feeds the lift pass), which is why
            // `for (v of xs)` inside `() => {}` was a capture of an
            // identifier no scope could ever hold, while the same loop
            // in a named fn was fine.
            let saved = bound.len();
            bound.push(i_ident.clone());
            walk_expr(ast, *elem_expr, bound, out);
            bound.push(var_name.clone());
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Labeled { body, .. } => walk_stmt(ast, body, bound, out),
        Stmt::Throw(eid) => walk_expr(ast, *eid, bound, out),
        Stmt::Try {
            body,
            catch_param,
            catch_type: _,
            had_catch: _,
            catch_body,
            finally_body,
        } => walk_try(ast, body, catch_param, catch_body, finally_body, bound, out),
        Stmt::FnDecl { params, body, .. } => walk_fn_decl(ast, params, body, bound, out),
        Stmt::TypeDecl { .. } => {}
        Stmt::ClassDecl {
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => walk_class_decl(
            ast,
            parent,
            ctor,
            methods,
            static_methods,
            static_init,
            bound,
            out,
        ),
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                walk_stmt(ast, inner, bound, out);
            }
        }
    }
}

/// Names that are pre-bound globals — they should never count as
/// closure captures even when they appear as bare idents inside an
/// arrow body. Currently the runtime-provided print / namespace
/// objects. Kept in sync with `check.rs`'s `type_of(Expr::Ident)`
/// fallback list. `pub(crate)` (re-exported as
/// `ast::free_vars_is_global_name`): the nested-class hoist pass
/// asks it whether an `extends <name>` parent resolves globally,
/// and the let-widen pre-pass classifies `Ns.method(...)` rhs
/// shapes against it.
pub(crate) use super::free_vars_globals::is_global_name;
