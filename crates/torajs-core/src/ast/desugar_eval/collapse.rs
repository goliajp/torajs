//! Collapse analysis for the value-position eval walk: which parsed
//! sources are provably unobservable, so the call can fold to its
//! completion value without running anything. Moved verbatim from
//! `desugar_eval.rs` (r334 blade 6 sibling split).

use super::super::{Ast, Expr, Stmt};

/// A declaration whose evaluation nothing can observe **in a strict
/// direct eval**: a `let` / `var` whose initializer is an identifier
/// read or a literal, or a function declaration. `Stmt::Multi` of such
/// declarations counts too — the parser expands `var x, y` into one.
/// The initializer bound is what keeps the collapse exact; a call, a
/// member access, anything that could run code disqualifies the whole
/// source.
pub(super) fn is_effect_free_decl(s: &Stmt, ast: &Ast) -> bool {
    match s {
        // `Uninit` is the parser's sentinel for `var x;` with no
        // initializer at all — the most effect-free init there is.
        Stmt::LetDecl { init, .. } => matches!(
            ast.exprs.get(init.0 as usize),
            Some(Expr::Uninit | Expr::Ident(_) | Expr::String(_) | Expr::Number(_))
        ),
        Stmt::FnDecl { .. } => true,
        Stmt::Multi(list) => list.iter().all(|s| is_effect_free_decl(s, ast)),
        _ => false,
    }
}

/// A statement whose completion is *empty*, whose evaluation has no
/// observable effect in ANY scope, and which provably terminates:
/// empty blocks, control flow whose paths are decided by literal
/// conditions and reach only empty-and-effect-free statements. `(0,
/// eval)("{}")` and `(0, eval)("for(false;false;false);")` are
/// `undefined` by §14.5.1, and test262's cptn-nrml-empty family spells
/// exactly these.
///
/// Three boundaries are load-bearing:
/// - a literal EXPRESSION statement is value-producing (`eval("1;
///   2;")` is `2`, not undefined), so `Stmt::Expr` never qualifies;
/// - a loop qualifies only when its literal condition is FALSY — a
///   truthy `while (true);` never terminates, and collapsing it to
///   `undefined` would change that;
/// - an `if` over a truthy literal completes with its taken branch's
///   completion, so the branch actually reached must itself qualify.
pub(super) fn completes_empty_effect_free(s: &Stmt, ast: &Ast) -> bool {
    match s {
        Stmt::Block(b) | Stmt::Multi(b) => b.iter().all(|s| completes_empty_effect_free(s, ast)),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => match lit_truth(*cond, ast) {
            Some(true) => completes_empty_effect_free(then_branch, ast),
            Some(false) => else_branch
                .as_deref()
                .is_none_or(|e| completes_empty_effect_free(e, ast)),
            None => false,
        },
        Stmt::While { cond, .. } => lit_truth(*cond, ast) == Some(false),
        Stmt::DoWhile { body, cond } => {
            lit_truth(*cond, ast) == Some(false) && completes_empty_effect_free(body, ast)
        }
        Stmt::For {
            init, cond, body, ..
        } => {
            // The step never runs under a falsy condition; a missing
            // condition is an infinite loop and disqualifies. The
            // init is NOT a completion position, so a literal
            // expression statement there is fine — its value is
            // discarded, only its effects (none) matter.
            init.as_deref().is_none_or(|i| match i {
                Stmt::Expr(e) => lit_truth(*e, ast).is_some(),
                other => completes_empty_effect_free(other, ast),
            }) && cond.is_some_and(|c| lit_truth(c, ast) == Some(false))
                && completes_empty_effect_free(body, ast)
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            // Only the trivially-empty switch: any case body might be
            // the one dispatched to, and a value-producing body would
            // become the completion.
            lit_truth(*scrutinee, ast).is_some() && cases.is_empty() && default.is_none()
        }
        _ => false,
    }
}

/// The boolean a literal coerces to, or `None` for anything that is
/// not a literal. Mirrors §7.1.2 ToBoolean over the literal shapes the
/// parser can produce directly.
fn lit_truth(e: super::super::ExprId, ast: &Ast) -> Option<bool> {
    match ast.exprs.get(e.0 as usize)? {
        Expr::Bool(b) => Some(*b),
        Expr::Number(n) => Some(*n != 0.0 && !n.is_nan()),
        Expr::String(s) => Some(!s.is_empty()),
        Expr::Null => Some(false),
        _ => None,
    }
}

/// The names a declaration source binds — what a sloppy indirect eval
/// would put on the global.
pub(super) fn decl_names(stmts: &[Stmt], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            Stmt::LetDecl { name, .. } => out.push(name.clone()),
            Stmt::FnDecl { name, .. } => out.push(name.clone()),
            Stmt::Multi(list) => decl_names(list, out),
            _ => {}
        }
    }
}

/// Could the program reach a global binding of this name? Any
/// identifier spelling it, any member access naming it (`this.x`), or
/// any string literal equal to it (`'x' in this`,
/// `getOwnPropertyDescriptor(this, "x")`) counts. The scan stops at
/// `limit` — the arena length before the current eval source was
/// parsed in — so the source's own uses of its own names do not veto
/// the collapse. Garbage expressions left by earlier collapses are
/// scanned too, which can only over-decline: the safe direction.
pub(super) fn name_mentioned(ast: &Ast, name: &str, limit: usize) -> bool {
    ast.exprs[..limit.min(ast.exprs.len())]
        .iter()
        .any(|e| match e {
            Expr::Ident(n) => n == name,
            Expr::Member { name: n, .. } | Expr::OptChain { name: n, .. } => n == name,
            Expr::String(s) => s == name,
            _ => false,
        })
}
