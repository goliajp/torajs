//! RFC 20260805-async-fn-state-machine blade 2 (D2) — async function
//! VALUES: `async (a) => { … }` and `async function (a) { … }`, both
//! parsed as `Expr::ArrowFn` and marked in `Ast::async_fn_value_exprs`.
//!
//! The decl rewrite in [`super::desugar_async_sm`] turns a body into a
//! `function*` and a wrapper that drives it. A value gets the same two
//! halves, spelled as expressions: the body becomes a generator
//! EXPRESSION — a fresh `ArrowFn` marked in `gen_fn_exprs`, which
//! `hoist_gen_fn_exprs` lifts to a top-level `function* __genexpr_N`
//! and, when the body captures enclosing locals, routes through its
//! wrapper closure (snapshot or by-reference, the hoist's policy) —
//! and the original slot becomes `(params) => __async_drive(() =>
//! <generator>(params))`, an ordinary arrow `lift_arrow_fns` gives an
//! env. This pass therefore runs BEFORE the hoist.
//!
//! Not moved yet, each keeping today's synchronous lowering: a body
//! that reads `this` (an arrow's `this` is lexical, a hoisted
//! generator's is its own — RFC 20260717's fn-expr receiver channel
//! and the generator hoist would have to agree first; `ThisRead::
//! Blocks`); a body declaring a function or a class (the hoist's
//! capture snapshot does not reach a nested declaration, and a class
//! lifted as a generator field registers twice — rotation 641's two
//! red shapes, `declares_fn_or_class`); a parameter the parser
//! synthesized (destructuring prefix — the hoist re-registers that
//! count for declared generators only); and everything the decl gate
//! refuses (`desugar_async_sm_gate`).

use super::desugar_async_sm::{
    build_drive_call_to, generator_params, rewrite_body, wrapper_params,
};
use super::desugar_async_sm_gate::{ThisRead, blocks_move, declares_fn_or_class};
use super::{Ast, Expr, ExprId, GenFnExprInfo, GenFnExprKind, Stmt};

/// Rewrite every movable async function value. Answers whether any
/// moved, so the caller injects the driver.
pub(super) fn rewrite_async_fn_values(ast: &mut Ast) -> bool {
    // Deterministic order: the rewrites append to the arena.
    let mut ids: Vec<ExprId> = ast.async_fn_value_exprs.iter().copied().collect();
    ids.sort_by_key(|e| e.0);
    let mut moved_any = false;
    for eid in ids {
        let Expr::ArrowFn {
            params,
            return_type: _,
            body,
        } = ast.get_expr(eid).clone()
        else {
            continue;
        };
        if params.iter().any(|p| p.name.starts_with("__"))
            || blocks_move(ast, &params, &body, ThisRead::Blocks)
            || declares_fn_or_class(ast, &body)
        {
            continue;
        }
        let mut moved = body.clone();
        if !rewrite_body(ast, &mut moved) {
            continue;
        }
        let generator = ast.add_expr(Expr::ArrowFn {
            params: generator_params(&params),
            // P10.7 default-Any generator, as for a moved declaration.
            return_type: Some("any".into()),
            body: moved,
        });
        ast.gen_fn_exprs.insert(
            generator,
            GenFnExprInfo {
                kind: GenFnExprKind::Generator,
                destr_prefix: 0,
            },
        );
        let drive = build_drive_call_to(ast, generator, &params);
        let wrapper_params = wrapper_params(ast, &params);
        ast.exprs[eid.0 as usize] = Expr::ArrowFn {
            params: wrapper_params,
            return_type: Some("Promise<any>".into()),
            body: vec![Stmt::Return(Some(drive))],
        };
        // The mark STAYS in `async_fn_value_exprs` (the lifted closure's
        // %AsyncFunction% flavour, the §15.8.4 param-throw posture);
        // only `desugar_async`'s value lane consults this set.
        ast.async_sm_value_exprs.insert(eid);
        moved_any = true;
    }
    moved_any
}
