//! Default-parameter TDZ (rotation 275 刀 3) — §8.6.2
//! IteratorBindingInitialization order: a parameter's default
//! initializer runs in the params environment where the parameter
//! itself and every LATER parameter are still uninitialized, so a
//! bare read of either must throw a ReferenceError at call time
//! (`function f(x = x)` / `f(a = b, b)` — the test262
//! `dflt-params-ref-self` / `-ref-later` families).
//!
//! tr evaluates defaults at the CALL SITE (`apply_default_args`
//! padding), where such a name resolves to the outer scope (or
//! nothing) instead of the params environment — `x = x` silently
//! read the outer `x`. This pass rewrites the default expression IN
//! PLACE (arena slot overwrite, the `ast_desugar_promise_try`
//! pattern) to an immediately-invoked zero-param arrow that throws:
//!
//! ```text
//! x = x   ⇒   x = (() => { throw new ReferenceError("...") })()
//! ```
//!
//! Every consumer of the default ExprId (call-site padding, spread
//! expansion) picks the rewrite up for free, and a call that DOES
//! supply the argument never evaluates it — exactly the spec's
//! lazy-initializer semantics.
//!
//! Scope of the rewrite: only a default whose whole expression is a
//! BARE identifier naming the parameter itself or a later one — the
//! one shape whose evaluation provably hits the TDZ. A compound
//! initializer (`x = x + 1`) or a closure capture (`a = () => b, b`)
//! stays untouched: a closure may legally run after the binding
//! initializes, and a compound read cannot be judged without
//! evaluation order analysis. Runs after `desugar_classes` (methods
//! are flat `__cm_` FnDecls; hidden leading params never collide
//! with user names) and before `lift_arrow_fns` (the minted arrow
//! needs its env like any other).

use super::{Ast, Expr, ExprId, Param, Stmt};

pub(crate) fn run(ast: &mut Ast) {
    let mut hits: Vec<(ExprId, String)> = Vec::new();
    // Async functions take the rewrite too. §15.8.4 step 3 wants a
    // param-initializer throw as a REJECTION of the returned promise,
    // and that is what it becomes: the minted IIFE is a closed function
    // literal, so `materialize_expr_defaults` moves it off the call-site
    // pad into a body guard — inside the async `try` for a
    // synchronously lowered body, inside the driver's `try` for a moved
    // one — and the throw is caught there. Left on the pad (the
    // rotation-275 skip) the bare `x` resolved to the CALLER's `x` and
    // the body ran: the 18 `dflt-params-ref-{self,later}` async cases
    // (642-07). Async generators were never skipped — their §15.6.2
    // param throw is synchronous, which the pad gives for free.
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, .. } = s {
            collect_param_hits(ast, params, &mut hits);
        }
    }
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, .. } = e {
            collect_param_hits(ast, params, &mut hits);
        }
    }
    for (eid, name) in hits {
        rewrite_to_throw_iife(ast, eid, &name);
    }
}

/// Record `(default eid, referenced name)` for every default that is
/// a bare Ident naming its own parameter or a later sibling.
fn collect_param_hits(ast: &Ast, params: &[Param], hits: &mut Vec<(ExprId, String)>) {
    for (i, p) in params.iter().enumerate() {
        let Some(d) = p.default else {
            continue;
        };
        let Expr::Ident(n) = ast.get_expr(d) else {
            continue;
        };
        if params[i..].iter().any(|q| q.name == *n) {
            hits.push((d, n.clone()));
        }
    }
}

/// Overwrite the default's arena slot with
/// `(() : any => { throw __new_ReferenceError("...") })()`.
///
/// The factory call goes straight at the `desugar_classes`-synthesized
/// `__new_ReferenceError` (the injected native-error class) — this
/// pass runs after the `Expr::New`-rewriting passes, so a fresh `New`
/// node would reach the checker unresolved.
fn rewrite_to_throw_iife(ast: &mut Ast, eid: ExprId, name: &str) {
    let call = tdz_throw_iife(ast, name, None);
    ast.exprs[eid.0 as usize] = call;
}

/// The node itself: `(() : any => { [<before>;] throw
/// __new_ReferenceError("Cannot access '<name>' before initialization")
/// })()`, returned rather than stored so the caller decides which slot
/// it overwrites. `before` is an expression to evaluate ahead of the
/// throw — the right-hand side of a dead-zone WRITE (§13.15.2 evaluates
/// the value before PutValue raises). Shared with the machine-body
/// dead zone (`desugar_generators_tdz`, RFC 20260915 item 2).
pub(super) fn tdz_throw_iife(ast: &mut Ast, name: &str, before: Option<ExprId>) -> Expr {
    let msg = ast.add_expr(Expr::String(
        format!("Cannot access '{name}' before initialization.").into(),
    ));
    let factory = ast.add_expr(Expr::Ident("__new_ReferenceError".to_string()));
    let err = ast.add_expr(Expr::Call {
        callee: factory,
        args: vec![msg],
    });
    let mut body = Vec::new();
    if let Some(b) = before {
        body.push(Stmt::Expr(b));
    }
    body.push(Stmt::Throw(err));
    let arrow = ast.add_expr(Expr::ArrowFn {
        params: vec![],
        return_type: Some("any".to_string()),
        body,
    });
    Expr::Call {
        callee: arrow,
        args: vec![],
    }
}
