//! Sloppy-goal implicit globals — the third member of the goal-triage
//! family (`delete <bare name>` / readonly-global writes next door).
//!
//! §9.1.1.4.6 SetMutableBinding + §6.2.5.6 PutValue: in sloppy code an
//! assignment to an unresolvable name CREATES a global binding at run
//! time (`__x = 1` then `typeof __x` is `"number"`); before the write
//! runs, the name simply does not resolve (`typeof __x` is
//! `"undefined"`). The checker hard-rejects such writes for
//! `__`-prefixed names (its compiler-synthesized carve-out) and would
//! give the strict runtime-ReferenceError posture to the rest — both
//! wrong under the sloppy goal.
//!
//! Statically decidable here, same as the siblings: walk the program
//! for assignments whose target is an identifier it never declares,
//! and synthesize one hoisted `var <name>;` per name at the top
//! level. A hoisted uninitialized var IS the observable shape of the
//! not-yet-written implicit global (`undefined` on read, `typeof`
//! answers `"undefined"`), and after the write both agree.
//!
//! The walk is a TREE walk, not an arena scan, because strictness is
//! positional: a function whose directive prologue says `"use
//! strict"` gives its assignments the strict posture (runtime
//! ReferenceError — `assert.throws(ReferenceError, fun)` over a
//! prologue'd body is a measured shape), so sites inside a strict
//! body must not seed a binding. Inherited strictness needs no
//! propagation here — the parser materializes a directive into every
//! strict-by-inheritance body (`parser::strict_directive`).
//!
//! Declines and exclusions (each measured, none speculative):
//! - a program with `with` keeps the reject — §14.11 resolves a
//!   body's names through the scope object, so an assignment there
//!   is not evidence of an implicit global (`with (o) { foo = 42 }`
//!   must NOT create `foo`);
//! - a name the program also `delete`s stays out — a var binding is
//!   non-configurable where the implicit global's property must
//!   delete away (`x = 1; delete x; x` wants the ReferenceError);
//! - known builtin globals keep the recorded reject next to
//!   `check_assign_ident`'s carve-out, the §19.1 readonly names never
//!   reach this pass (the sibling already folded their writes), and
//!   the contextual keywords (`let` / `yield` / `await` / `static`)
//!   stay out — a synthesized `var let;` is not a spelling the rest
//!   of the pipeline accepts.
//!
//! Runs right after the readonly sibling, before the checker.

use super::sloppy_this_prologue::has_use_strict_directive;
use super::{Ast, Expr, ExprId, Stmt};

pub fn synthesize_sloppy_implicit_globals(ast: &mut Ast) {
    if !ast.sloppy_script_goal || ast.has_with_stmt {
        return;
    }
    let mut declared = std::collections::HashSet::new();
    super::delete_bare_name::collect_declared_names(&ast.stmts, &mut declared);
    let mut names: Vec<String> = Vec::new();
    walk_block(ast, &ast.stmts, &declared, &mut names);
    let decls: Vec<Stmt> = names
        .into_iter()
        .map(|name| {
            let init = ast.add_expr(Expr::Uninit);
            ast.sloppy_implicit_global_names.insert(name.clone());
            Stmt::LetDecl {
                mutable: true,
                name,
                type_ann: None,
                init,
                is_var: true,
            }
        })
        .collect();
    ast.stmts.splice(0..0, decls);
}

fn admit(ast: &Ast, n: &str, declared: &std::collections::HashSet<String>, out: &mut Vec<String>) {
    if declared.contains(n)
        || out.iter().any(|seen| seen == n)
        || ast.sloppy_deleted_bare_names.contains(n)
        || crate::check::is_known_builtin_global(n)
        || matches!(
            n,
            "undefined"
                | "NaN"
                | "Infinity"
                | "eval"
                | "arguments"
                | "let"
                | "yield"
                | "await"
                | "static"
        )
    {
        return;
    }
    out.push(n.to_string());
}

fn walk_block(
    ast: &Ast,
    stmts: &[Stmt],
    declared: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    for s in stmts {
        walk_stmt(ast, s, declared, out);
    }
}

fn walk_stmt(
    ast: &Ast,
    s: &Stmt,
    declared: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    match s {
        // A strict body's assignments keep the strict posture — skip
        // the whole subtree (strictness is contagious downward, and
        // strict-by-inheritance bodies carry a parser-written
        // directive of their own).
        Stmt::FnDecl { body, .. } => {
            if !has_use_strict_directive(body, &ast.exprs) {
                walk_block(ast, body, declared, out);
            }
        }
        Stmt::Expr(e) | Stmt::Yield(e) | Stmt::Throw(e) | Stmt::Return(Some(e)) => {
            walk_expr(ast, *e, declared, out)
        }
        Stmt::YieldInto { value, .. } => walk_expr(ast, *value, declared, out),
        Stmt::LetDecl { init, .. } | Stmt::UsingDecl { init, .. } => {
            walk_expr(ast, *init, declared, out)
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, declared, out);
            walk_stmt(ast, then_branch, declared, out);
            if let Some(eb) = else_branch {
                walk_stmt(ast, eb, declared, out);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            walk_expr(ast, *cond, declared, out);
            walk_stmt(ast, body, declared, out);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                walk_stmt(ast, i, declared, out);
            }
            if let Some(c) = cond {
                walk_expr(ast, *c, declared, out);
            }
            if let Some(st) = step {
                walk_expr(ast, *st, declared, out);
            }
            walk_stmt(ast, body, declared, out);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            walk_expr(ast, *elem_expr, declared, out);
            walk_stmt(ast, body, declared, out);
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            walk_expr(ast, *parent, declared, out);
            walk_expr(ast, *sep, declared, out);
            walk_stmt(ast, body, declared, out);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            walk_expr(ast, *scrutinee, declared, out);
            for c in cases {
                walk_expr(ast, c.value, declared, out);
                walk_block(ast, &c.body, declared, out);
            }
            if let Some(d) = default {
                walk_block(ast, d, declared, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            walk_block(ast, body, declared, out);
            walk_block(ast, catch_body, declared, out);
            if let Some(fb) = finally_body {
                walk_block(ast, fb, declared, out);
            }
        }
        Stmt::Block(inner) | Stmt::Multi(inner) => walk_block(ast, inner, declared, out),
        Stmt::Labeled { body, .. } => walk_stmt(ast, body, declared, out),
        Stmt::ExportDecl { inner, .. } => {
            if let Some(i) = inner {
                walk_stmt(ast, i, declared, out);
            }
        }
        _ => {}
    }
}

fn walk_expr(
    ast: &Ast,
    eid: ExprId,
    declared: &std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    match ast.get_expr(eid) {
        Expr::Assign { target, value } => {
            if let Expr::Ident(n) = ast.get_expr(*target) {
                admit(ast, n, declared, out);
            } else {
                walk_expr(ast, *target, declared, out);
            }
            walk_expr(ast, *value, declared, out);
        }
        // A strict arrow body keeps its assignments out, same as
        // FnDecl above (arrows inherit strictness, and inherited
        // strict bodies carry a parser-written directive).
        Expr::ArrowFn { body, .. } => {
            if !has_use_strict_directive(body, &ast.exprs) {
                walk_block(ast, body, declared, out);
            }
        }
        Expr::Call { callee, args } => {
            walk_expr(ast, *callee, declared, out);
            for &a in args {
                walk_expr(ast, a, declared, out);
            }
        }
        Expr::OptCall { callee, args, .. } => {
            walk_expr(ast, *callee, declared, out);
            for &a in args {
                walk_expr(ast, a, declared, out);
            }
        }
        Expr::New { args, .. } => {
            for &a in args {
                walk_expr(ast, a, declared, out);
            }
        }
        Expr::NewDynamic { callee, args, .. } => {
            walk_expr(ast, *callee, declared, out);
            for &a in args {
                walk_expr(ast, a, declared, out);
            }
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            walk_expr(ast, *obj, declared, out)
        }
        Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
            walk_expr(ast, *obj, declared, out);
            walk_expr(ast, *index, declared, out);
        }
        Expr::BinOp { left, right, .. } | Expr::Sequence { left, right } => {
            walk_expr(ast, *left, declared, out);
            walk_expr(ast, *right, declared, out);
        }
        Expr::Nullish { lhs, rhs } => {
            walk_expr(ast, *lhs, declared, out);
            walk_expr(ast, *rhs, declared, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Delete { expr }
        | Expr::PostIncr { target: expr, .. }
        | Expr::As { expr, .. }
        | Expr::Spread { expr } => walk_expr(ast, *expr, declared, out),
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, declared, out);
            walk_expr(ast, *then_branch, declared, out);
            walk_expr(ast, *else_branch, declared, out);
        }
        Expr::Array(items) => {
            for &el in items {
                walk_expr(ast, el, declared, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, declared, out);
            }
        }
        _ => {}
    }
}
