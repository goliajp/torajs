//! The `__this` honest-reject gate — runs after the LAST
//! receiver-supplying rule.
//!
//! Every pass that can claim a function-expression `this` has run by
//! the time this looks (`bind_fnexpr_this_default` is the final one;
//! the pipeline calls this gate right after it). A closure mint still
//! CAPTURING `__this` from a scope that does not bind it is therefore
//! a function expression reading `this` in a position nobody claims.
//! Until now that program died in the checker with ``closure
//! `__closure_N` references unknown identifier `__this` `` — loud,
//! but it leaks two internal names and lands in the sweep's
//! unknown-ident cluster instead of the self-describing `not yet
//! supported:` bucket every other subset boundary uses. This gate
//! keeps the same accept/reject line and changes only the words.
//!
//! "Would the checker reject" is a scope question: the mint site must
//! have `__this` in scope. Three providers exist by this stage — a
//! `__this` parameter (`bind_this_param`, the class-method desugar),
//! a `__this` entry in the enclosing lifted body's `__env(...)`
//! capture list (that closure's own mint answers for its legality at
//! ITS mint site, so a local yes suffices), and a body-local
//! `let __this` (the default-binding and sloppy-prologue mints).
//!
//! The scan under-fires by design: a provider is honoured anywhere in
//! the enclosing body even where block scoping would hide it, and a
//! tree position the walkers miss keeps today's checker reject — but
//! it must never fire on a program that compiles. The tree shape
//! helpers are borrowed from [`super::desugar_with::walk`]; an arm
//! they lack costs an under-fire, never a wrong reject.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::{Ast, Expr, ExprId, Stmt};

/// The one line the reject prints (under the pipeline's
/// `not yet supported:` prefix) — fixed on purpose, so the sweep sees
/// one signature instead of a spread of per-case spellings, and free
/// of internal names on purpose, so a user can act on it.
const MESSAGE: &str = "fnexpr this in unclaimed receiver position: \
     a function expression reads 'this' where tr does not yet track \
     a receiver for it";

/// `Some(message)` = the program holds an unclaimed fn-expr `this`;
/// the pipeline prints it under `not yet supported:` and stops before
/// the checker can spell the same reject cryptically.
pub fn unclaimed_fnexpr_this(ast: &Ast) -> Option<String> {
    scan_list(ast, &ast.stmts, false)
}

fn scan_list(ast: &Ast, stmts: &[Stmt], provided: bool) -> Option<String> {
    let provided = provided || stmts.iter().any(stmt_declares_this);
    for s in stmts {
        if let Some(m) = scan_stmt(ast, s, provided) {
            return Some(m);
        }
    }
    None
}

fn scan_stmt(ast: &Ast, s: &Stmt, provided: bool) -> Option<String> {
    if let Stmt::FnDecl { params, body, .. } = s {
        let fn_provides = params.iter().any(|p| p.name == "__this")
            || params.first().is_some_and(|p| {
                p.name == "__env" && p.type_ann.as_deref().is_some_and(env_ann_has_this)
            });
        return scan_list(ast, body, fn_provides);
    }
    for root in stmt_exprs(s) {
        if let Some(m) = scan_expr(ast, root, provided) {
            return Some(m);
        }
    }
    for child in stmt_children_ref(s) {
        if let Some(m) = scan_stmt(ast, child, provided) {
            return Some(m);
        }
    }
    None
}

fn scan_expr(ast: &Ast, eid: ExprId, provided: bool) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } if !provided && captures.iter().any(|c| c == "__this") => {
            return Some(MESSAGE.to_string());
        }
        // An arrow inherits the enclosing `this` (§8.3.4) — its body
        // sees the same provision. `expr_children` deliberately stops
        // at arrow bodies, so this arm descends.
        Expr::ArrowFn { body, .. } => return scan_list(ast, body, provided),
        _ => {}
    }
    for c in expr_children(ast, eid) {
        if let Some(m) = scan_expr(ast, c, provided) {
            return Some(m);
        }
    }
    None
}

/// A `let` / `var` binding literally named `__this` anywhere in the
/// statement (nested blocks included, nested FUNCTIONS excluded —
/// those open their own scope and answer for themselves).
fn stmt_declares_this(s: &Stmt) -> bool {
    match s {
        Stmt::LetDecl { name, .. } => name == "__this",
        Stmt::FnDecl { .. } => false,
        other => stmt_children_ref(other).into_iter().any(stmt_declares_this),
    }
}

/// `__env(a|b|__this|c)` — the capture-list spelling
/// `bind_fnexpr_this_default` and the lift keep in the env param's
/// annotation.
fn env_ann_has_this(ann: &str) -> bool {
    ann.strip_prefix("__env(")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|caps| caps.split('|').any(|c| c == "__this"))
}
