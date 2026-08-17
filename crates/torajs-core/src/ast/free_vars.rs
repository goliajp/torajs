//! Free-variable analysis for arrow-fn bodies.
//!
//! Chunk 346 — extracted from ast.rs. The entry `free_vars_of_arrow`
//! and its three sibling-private walkers (`walk_stmt` / `walk_expr`
//! recursive arena traversal + `is_global_name` filter for built-in
//! receivers) form one logical unit and lift cleanly into a single
//! sibling. Caller is one site in ast.rs (line ~3037, inside the
//! arrow-fn lift pass) that resolves via the pub(super) marker on
//! free_vars_of_arrow.

use super::{Ast, Expr, ExprId, Param, Stmt};

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

/// Function declarations hoist: a reference anywhere in the holding
/// list — including before the declaration — resolves to the local
/// decl, never to an outer binding. Bind every direct FnDecl name
/// before walking the list's statements.
fn hoist_fn_decl_names(list: &[Stmt], bound: &mut Vec<String>) {
    for s in list {
        if let Stmt::FnDecl { name, .. } = s {
            if !bound.contains(name) {
                bound.push(name.clone());
            }
        }
    }
}

/// A nested `function` declaration's body walk. The decl's own name is
/// hoist-bound by the enclosing list walk. Its body is a scope of its
/// own: params and the decl's `arguments` quasi-binding bind, and
/// whatever stays free inside is free in the enclosing scope too — a
/// nested decl reading an outer local makes the enclosing closure
/// capture it. Ignoring the body (the pre-fix behavior) both reported
/// the decl's NAME as a phantom capture and hid its real ones.
///
/// `__this` binds too: a `function` declaration binds its own `this`
/// (§10.2.1.1 non-lexical [[ThisMode]]), so the body's `__this` — the
/// desugar_classes pass-2 spelling — is NOT free in the enclosing
/// scope. Without this bound entry an enclosing lifted closure carries
/// a phantom `__this` capture its definition scope cannot supply, and
/// the checker rejects the whole closure (unknown identifier) even
/// though the nested decl itself promotes fine.
fn walk_fn_decl(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    bound: &mut Vec<String>,
    out: &mut Vec<String>,
) {
    let saved = bound.len();
    for p in params {
        bound.push(p.name.clone());
    }
    bound.push("arguments".into());
    bound.push("__this".into());
    hoist_fn_decl_names(body, bound);
    for s in body {
        walk_stmt(ast, s, bound, out);
    }
    bound.truncate(saved);
}

fn walk_stmt(ast: &Ast, s: &Stmt, bound: &mut Vec<String>, out: &mut Vec<String>) {
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
            for c in cases {
                walk_expr(ast, c.value, bound, out);
                let saved = bound.len();
                hoist_fn_decl_names(&c.body, bound);
                for s in &c.body {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
            if let Some(db) = default {
                let saved = bound.len();
                hoist_fn_decl_names(db, bound);
                for s in db {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
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
        } => {
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
        Stmt::FnDecl { params, body, .. } => walk_fn_decl(ast, params, body, bound, out),
        Stmt::TypeDecl { .. } => {}
        Stmt::ClassDecl { .. } => {
            // desugar_classes runs before lift_arrow_fns; if a ClassDecl
            // somehow remains, ignore — its body has already been split
            // into FnDecls anyway.
        }
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

fn walk_expr(ast: &Ast, eid: ExprId, bound: &mut Vec<String>, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Elision => {}
        Expr::Ident(name) => {
            if is_global_name(name) {
                return;
            }
            if !bound.contains(name) && !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::String(_)
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => {}
        Expr::BinOp { left, right, .. } => {
            walk_expr(ast, *left, bound, out);
            walk_expr(ast, *right, bound, out);
        }
        Expr::Unary { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::Member { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::Call { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Assign { target, value } => {
            walk_expr(ast, *target, bound, out);
            walk_expr(ast, *value, bound, out);
        }
        Expr::Index { obj, index } => {
            walk_expr(ast, *obj, bound, out);
            walk_expr(ast, *index, bound, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            let saved = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            // A marked fn-expr (`function () {}` parsed into an
            // ArrowFn node and listed in `ast.fn_expr_exprs`) and an
            // object-literal method shorthand (incl. get/set —
            // `ast.objlit_method_exprs`) are FUNCTION-this: §10.2.1.2
            // binds their `this` at their own call site, so the
            // body's `__this` is not free in the enclosing scope
            // (same boundary as the Stmt::FnDecl arm). A plain arrow
            // stays lexical — its `__this` keeps riding up to the
            // enclosing function, which is what hands an arrow inside
            // a promoted fn-expr its receiver.
            if ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid) {
                bound.push("__this".into());
            }
            hoist_fn_decl_names(body, bound);
            for s in body {
                walk_stmt(ast, s, bound, out);
            }
            bound.truncate(saved);
        }
        Expr::Closure { captures, .. } => {
            // Already lifted (the arena is walked in index order, so an
            // inner lambda is a Closure by the time its encloser's
            // captures are computed): the captures referenced by an
            // already-lifted closure are themselves free in the current
            // arrow body if not bound here — EXCEPT `__this` on a
            // function-this closure (a marked fn-expr or an objlit
            // method shorthand; the eid survives the in-place lift, so
            // the marker sets still answer). That capture is the
            // promote protocol's marker — `fnexpr_this` /
            // `objlit_nominal` later turn it into a receiver param —
            // not a lexical need the enclosing scope must supply;
            // riding it up left the encloser with a stale `__this`
            // capture after the inner promote (unknown-identifier
            // reject). An ARROW's `__this` capture stays lexical and
            // keeps riding up.
            for c in captures {
                if c == "__this"
                    && (ast.fn_expr_exprs.contains(&eid) || ast.objlit_method_exprs.contains(&eid))
                {
                    continue;
                }
                if !bound.contains(c) && !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        // M5.1 — by the time arrow-fn lifting runs, classes have already
        // been desugared to functions (and `this` to `__this`). These
        // arms guard against an arrow body that lexically nests inside a
        // class method whose desugar hasn't completed; in practice we
        // run desugar_classes before lift_arrow_fns, so they're inert.
        Expr::This | Expr::NewTarget => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::NewDynamic { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, bound, out);
            walk_expr(ast, *then_branch, bound, out);
            walk_expr(ast, *else_branch, bound, out);
        }
        Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::Spread { expr }
        | Expr::As { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::InstanceOf { expr, rhs } => {
            walk_expr(ast, *expr, bound, out);
            // A BARE name on the right is resolved by the lowering's
            // static ladder, from the NAME — no value is ever
            // materialised, so it is not a free variable and must not
            // become a capture. (It could not be one before either:
            // the target used to be a `String`, invisible here.) A
            // larger target IS an expression and walks normally, which
            // is the whole point of widening this node.
            walk_expr(ast, *rhs, bound, out);
        }
        Expr::Sequence { left, right } => {
            walk_expr(ast, *left, bound, out);
            walk_expr(ast, *right, bound, out);
        }
        Expr::Nullish { lhs, rhs } => {
            walk_expr(ast, *lhs, bound, out);
            walk_expr(ast, *rhs, bound, out);
        }
        Expr::OptChain { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::OptIndex { obj, index } => {
            walk_expr(ast, *obj, bound, out);
            walk_expr(ast, *index, bound, out);
        }
        Expr::OptCall { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::PostIncr { target, .. } => walk_expr(ast, *target, bound, out),
    }
}
