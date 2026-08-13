//! Escape-analysis pass for stack-safe Array literals — chunk 356,
//! extracted from ast.rs.
//!
//! Pub entry `escape_analyze_array_literals` records qualifying
//! `let X = [...]` ExprIds in `ast.stack_array_literals`; ssa_lower
//! emits them as stack alloca + STATIC_LITERAL flag. Five private
//! walkers (`eal_walk_stmts`, `eal_recurse_into`, `eal_stmt_safe`,
//! `eal_expr_safe`, `eal_expr_uses_x`) implement the safety check.

use super::{Ast, Expr, ExprId, Stmt};

/// JS's `arguments` object is array-like, holds the actual passed
/// values, and changes per call site. A faithful implementation needs
/// runtime support (heterogeneous array, per-call materialization).
///
/// Plan A — escape analyzer for non-Spread Array literals bound by
/// `let X = [...]`. For every such let-decl in every fn body (top-
/// level or class method), verify that X is used only in
/// stack-safe shapes within the rest of the body:
///
///   - `X.length`         (read)
///   - `X[i] / X[i] = v`  (read or write — i may be any expression
///                         that doesn't itself escape X)
///
/// Anything else — bare `X` reference (return, fn arg, store to
/// outer slot, alias to another let, throw, container element),
/// `X.foo` for any name other than "length", `X.method()`,
/// `X?.foo` — disqualifies the literal. The qualifying ExprId is
/// recorded in `ast.stack_array_literals`; ssa_lower emits these
/// as stack alloca + STATIC_LITERAL flag (rc_inc / rc_dec /
/// arr_drop all no-op via the existing flag pathway, so no heap
/// alloc + no per-call drop).
///
/// Runs after all desugars (so closure-lift visibility, arguments
/// rewrites, split-for-i fusion etc are already settled) so the
/// verifier sees the final shape.
/// False negatives stay heap (correct, just slower); false
/// positives would be silent UAF — bias every uncertain shape
/// toward false.
pub fn escape_analyze_array_literals(ast: &mut Ast) {
    // K.3b coherence (rotation 253) — a top-level binding a named-fn
    // body references gets PROMOTED to a data global (the ast_refs
    // gate the checker and the K.3 collect both consult), and the
    // K.4 init stores the literal's cell pointer into that slot. A
    // stack-alloca'd cell behind a global slot is exactly the silent
    // UAF this pass's bias rule forbids — the fn's read decodes main
    // frame layout (found live: an annotated `const flags: boolean[]`
    // answered its elements differently under iter and release
    // builds). The FnDecl arm below can't see this (it treats fn
    // bodies as separate scopes, and hoisting lets a fn DECLARED
    // BEFORE the let read it — outside the trailing window), so
    // consult the same table the promote decision uses: any name a
    // named-fn body references stays heap. Over-approximation
    // (fn-local shadows count) is the safe direction.
    let named_fn_refs = crate::ast_refs::toplevel_binding_refs(ast).named_fn_refs;
    let mut found: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    let stmts = ast.stmts.clone();
    eal_walk_stmts(ast, &stmts, &mut found, &named_fn_refs);
    ast.stack_array_literals = found;
}

fn eal_walk_stmts(
    ast: &Ast,
    stmts: &[Stmt],
    found: &mut std::collections::HashSet<ExprId>,
    named_fn_refs: &std::collections::HashSet<String>,
) {
    // Pass 1: at this level, check each `let X = [...]` against the
    // stmts that follow it (in source order — `let` is in scope from
    // its decl to end of block).
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::LetDecl { name, init, .. } = s
            && !named_fn_refs.contains(name)
            && let Expr::Array(els) = ast.get_expr(*init)
            && !els.is_empty()
            && !els
                .iter()
                .any(|e| matches!(ast.get_expr(*e), Expr::Spread { .. }))
        {
            // The array literal `init` is X's value. Verify X is
            // stack-safe in stmts[i+1..].
            let trailing = &stmts[i + 1..];
            if trailing.iter().all(|s| eal_stmt_safe(ast, s, name)) {
                found.insert(*init);
            }
        }
    }
    // Pass 2: recurse into every nested stmt list.
    for s in stmts {
        eal_recurse_into(ast, s, found, named_fn_refs);
    }
}

fn eal_recurse_into(
    ast: &Ast,
    s: &Stmt,
    found: &mut std::collections::HashSet<ExprId>,
    named_fn_refs: &std::collections::HashSet<String>,
) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => eal_walk_stmts(ast, inner, found, named_fn_refs),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            eal_recurse_into(ast, then_branch, found, named_fn_refs);
            if let Some(eb) = else_branch {
                eal_recurse_into(ast, eb, found, named_fn_refs);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            eal_recurse_into(ast, body, found, named_fn_refs)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                eal_recurse_into(ast, i, found, named_fn_refs);
            }
            eal_recurse_into(ast, body, found, named_fn_refs);
        }
        Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            eal_recurse_into(ast, body, found, named_fn_refs)
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                eal_walk_stmts(ast, &c.body, found, named_fn_refs);
            }
            if let Some(db) = default {
                eal_walk_stmts(ast, db, found, named_fn_refs);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            eal_walk_stmts(ast, body, found, named_fn_refs);
            eal_walk_stmts(ast, catch_body, found, named_fn_refs);
            if let Some(fb) = finally_body {
                eal_walk_stmts(ast, fb, found, named_fn_refs);
            }
        }
        Stmt::FnDecl { body, .. } => eal_walk_stmts(ast, body, found, named_fn_refs),
        Stmt::ClassDecl { methods, .. } => {
            for m in methods {
                eal_walk_stmts(ast, &m.body, found, named_fn_refs);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                eal_recurse_into(ast, inner, found, named_fn_refs);
            }
        }
        Stmt::Labeled { body, .. } => eal_recurse_into(ast, body, found, named_fn_refs),
        _ => {}
    }
}

fn eal_stmt_safe(ast: &Ast, s: &Stmt, x_name: &str) -> bool {
    match s {
        Stmt::Expr(eid) => eal_expr_safe(ast, *eid, x_name),
        Stmt::Throw(eid) => eal_expr_safe(ast, *eid, x_name),
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            // yield emits the value to the caller — escape.
            !eal_expr_uses_x(ast, *eid, x_name)
        }
        Stmt::Return(Some(eid)) => {
            // X returned → escape (any reference at all). We use the
            // stricter `uses_x` check here because X[i] returning the
            // i64 element is fine; X bare in the return is escape.
            // eal_expr_safe handles both: bare X = false, X[i] = true.
            eal_expr_safe(ast, *eid, x_name)
        }
        Stmt::Return(None) | Stmt::Break(_) | Stmt::Continue(_) => true,
        Stmt::Labeled { body, .. } => eal_stmt_safe(ast, body, x_name),
        Stmt::LetDecl { name, init, .. } | Stmt::UsingDecl { name, init, .. } => {
            // `let Y = X[i]` is fine (Y holds an element value);
            // `let Y = X` would be escape (caught by eal_expr_safe).
            // The new let's name shadows X in the body if same name;
            // we don't need to handle shadowing specially since once
            // X is shadowed the new binding takes over.
            let _ = name;
            eal_expr_safe(ast, *init, x_name)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            eal_expr_safe(ast, *cond, x_name)
                && eal_stmt_safe(ast, then_branch, x_name)
                && else_branch
                    .as_deref()
                    .map_or(true, |e| eal_stmt_safe(ast, e, x_name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            eal_expr_safe(ast, *cond, x_name) && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            init.as_deref()
                .map_or(true, |i| eal_stmt_safe(ast, i, x_name))
                && cond.map_or(true, |c| eal_expr_safe(ast, c, x_name))
                && step.map_or(true, |st| eal_expr_safe(ast, st, x_name))
                && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            eal_expr_safe(ast, *parent, x_name)
                && eal_expr_safe(ast, *sep, x_name)
                && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => eal_expr_safe(ast, *elem_expr, x_name) && eal_stmt_safe(ast, body, x_name),
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            eal_expr_safe(ast, *scrutinee, x_name)
                && cases.iter().all(|c| {
                    eal_expr_safe(ast, c.value, x_name)
                        && c.body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                })
                && default
                    .as_ref()
                    .map_or(true, |db| db.iter().all(|s| eal_stmt_safe(ast, s, x_name)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                && catch_body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                && finally_body
                    .as_ref()
                    .map_or(true, |fb| fb.iter().all(|s| eal_stmt_safe(ast, s, x_name)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| eal_stmt_safe(ast, s, x_name))
        }
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .map_or(true, |s| eal_stmt_safe(ast, s, x_name)),
    }
}

fn eal_expr_safe(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Elision => true,
        Expr::Ident(n) => n != x_name, // bare X is escape
        Expr::Member { obj, name } => {
            // X.length is the only allowed Member shape on X.
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return name == "length";
            }
            eal_expr_safe(ast, *obj, x_name)
        }
        Expr::Index { obj, index } => {
            // X[i] is allowed in any context. The index expression is
            // recursively checked (would not be a valid place to cite
            // X bare since X.length and X[k] are the only allowed
            // shapes — bare X in index expr would fail `Ident(n)` arm).
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return eal_expr_safe(ast, *index, x_name);
            }
            eal_expr_safe(ast, *obj, x_name) && eal_expr_safe(ast, *index, x_name)
        }
        Expr::Call { callee, args } => {
            eal_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| eal_expr_safe(ast, *a, x_name))
        }
        Expr::Assign { target, value } => {
            eal_expr_safe(ast, *target, x_name) && eal_expr_safe(ast, *value, x_name)
        }
        Expr::BinOp { left, right, .. } => {
            eal_expr_safe(ast, *left, x_name) && eal_expr_safe(ast, *right, x_name)
        }
        Expr::Unary { expr, .. } => eal_expr_safe(ast, *expr, x_name),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            eal_expr_safe(ast, *cond, x_name)
                && eal_expr_safe(ast, *then_branch, x_name)
                && eal_expr_safe(ast, *else_branch, x_name)
        }
        Expr::Array(els) => els.iter().all(|e| eal_expr_safe(ast, *e, x_name)),
        Expr::ObjectLit { fields } => fields.iter().all(|(_, e)| eal_expr_safe(ast, *e, x_name)),
        Expr::Spread { expr } => eal_expr_safe(ast, *expr, x_name),
        Expr::Nullish { lhs, rhs } => {
            eal_expr_safe(ast, *lhs, x_name) && eal_expr_safe(ast, *rhs, x_name)
        }
        Expr::OptChain { obj, .. } => {
            // X?.foo — disqualify; we'd have to permit only X?.length
            // and recursive analysis that's not worth the rare usage.
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            eal_expr_safe(ast, *obj, x_name)
        }
        Expr::OptIndex { obj, index } => {
            // X?.[i] — same disqualify story as OptChain.
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            eal_expr_safe(ast, *obj, x_name) && eal_expr_safe(ast, *index, x_name)
        }
        Expr::OptCall { callee, args } => {
            // X?.(…) — same story as Call: callee + args recurse.
            eal_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| eal_expr_safe(ast, *a, x_name))
        }
        Expr::PostIncr { target, .. } => eal_expr_safe(ast, *target, x_name),
        Expr::TypeOf { expr } | Expr::Delete { expr } => eal_expr_safe(ast, *expr, x_name),
        Expr::InstanceOf { expr, rhs } => {
            eal_expr_safe(ast, *expr, x_name) && eal_expr_safe(ast, *rhs, x_name)
        }
        Expr::As { expr, .. } => eal_expr_safe(ast, *expr, x_name),
        Expr::Sequence { left, right } => {
            eal_expr_safe(ast, *left, x_name) && eal_expr_safe(ast, *right, x_name)
        }
        Expr::Closure { captures, .. } => {
            // Closure captures = list of outer-scope names captured.
            // If X is captured, the lifted fn body could escape it.
            !captures.iter().any(|n| n == x_name)
        }
        Expr::ArrowFn { .. } => {
            // Arrow not yet lifted (would have been ArrowFn pre-lift)
            // — conservative: any arrow could capture X. Bail.
            false
        }
        Expr::New { args, .. } => args.iter().all(|a| eal_expr_safe(ast, *a, x_name)),
        Expr::NewDynamic { callee, args } => {
            eal_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| eal_expr_safe(ast, *a, x_name))
        }
        Expr::Super { args } => args.iter().all(|a| eal_expr_safe(ast, *a, x_name)),
        Expr::This
        | Expr::NewTarget
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::Regex { .. } => true,
    }
}

fn eal_expr_uses_x(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    !eal_expr_safe(ast, eid, x_name)
}
