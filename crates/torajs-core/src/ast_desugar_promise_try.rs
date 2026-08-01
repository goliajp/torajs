//! `Promise.try(f, ...args)` desugar (ES2025 §27.2.4.8, test262
//! `promise-try` family).
//!
//! The spec's shape — call `f` SYNCHRONOUSLY, resolve the returned
//! value (thenable-flattening included), turn a synchronous throw
//! into a rejection — is exactly what an inline try/catch around the
//! existing `Promise.resolve` / `Promise.reject` statics expresses,
//! so the rewrite mints an immediately-invoked zero-param arrow:
//!
//! ```text
//! Promise.try(f, a1, a2)
//!   ⇒ (() => { try { return Promise.resolve(f(a1, a2)) }
//!              catch (__ptry_e_N) { return Promise.reject(__ptry_e_N) } })()
//! ```
//!
//! Everything downstream is existing substrate: the arrow rides
//! `lift_arrow_fns` (capturing `f` / the argument expressions'
//! free variables through the normal env machinery), the inner call
//! rides the whole call pipeline whatever shape `f` is, and the two
//! statics ride the promise-static lanes. The pass therefore MUST
//! run before `lift_arrow_fns` (all three pipeline drivers place it
//! with the other `desugar_*` passes).
//!
//! Arena rewrite in place (the `ast_desugar_builtin_new` pass
//! family's pattern): the Call expr's eid is overwritten, so parent
//! references stay valid. The catch param embeds the eid to stay
//! collision-free under nesting. A zero-arg `Promise.try()` keeps
//! the loud no-member reject — the spec throws TypeError when the
//! callback is not callable, and the static lane has no runtime
//! callable-check station yet; loud beats a silently-minted promise.

use crate::ast::{Ast, Expr, ExprId, Stmt};

pub(crate) fn run(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let eid = ExprId(i as u32);
        let Expr::Call { callee, args } = ast.get_expr(eid) else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if name != "try" {
            continue;
        }
        let Expr::Ident(ns) = ast.get_expr(*obj) else {
            continue;
        };
        if ns != "Promise" {
            continue;
        }
        let Some((&f, rest)) = args.split_first() else {
            continue;
        };
        let rest: Vec<ExprId> = rest.to_vec();

        // try body: return Promise.resolve(f(rest...))
        let inner_call = ast.add_expr(Expr::Call {
            callee: f,
            args: rest,
        });
        let promise_a = ast.add_expr(Expr::Ident("Promise".to_string()));
        let resolve_m = ast.add_expr(Expr::Member {
            obj: promise_a,
            name: "resolve".to_string(),
        });
        let resolve_call = ast.add_expr(Expr::Call {
            callee: resolve_m,
            args: vec![inner_call],
        });

        // catch body: return Promise.reject(__ptry_e_N)
        let catch_name = format!("__ptry_e_{i}");
        let err_ref = ast.add_expr(Expr::Ident(catch_name.clone()));
        let promise_b = ast.add_expr(Expr::Ident("Promise".to_string()));
        let reject_m = ast.add_expr(Expr::Member {
            obj: promise_b,
            name: "reject".to_string(),
        });
        let reject_call = ast.add_expr(Expr::Call {
            callee: reject_m,
            args: vec![err_ref],
        });

        let arrow = ast.add_expr(Expr::ArrowFn {
            params: vec![],
            return_type: None,
            body: vec![Stmt::Try {
                body: vec![Stmt::Return(Some(resolve_call))],
                had_catch: true,
                catch_param: Some(catch_name),
                catch_type: None,
                catch_body: vec![Stmt::Return(Some(reject_call))],
                finally_body: None,
            }],
        });
        ast.exprs[i] = Expr::Call {
            callee: arrow,
            args: vec![],
        };
    }
}
