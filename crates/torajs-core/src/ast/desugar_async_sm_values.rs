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
//! A body that reads `this` moves the way a declaration's does
//! (`desugar_async_sm_this`): the generator takes the receiver as its
//! leading parameter and the wrapper hands its own `this` in — an
//! arrow's lexical one, or a function expression's call-site one,
//! each bound afterwards exactly as the unmoved body's would have
//! been. The parameter is `__asyncrecv`, not `__genrecv`: the hoist
//! reads a leading `__genrecv` as the fn-expr receiver-seeding channel
//! (r295) and refuses it beside captures, while this receiver is an
//! ordinary argument the wrapper passes.
//!
//! Not moved yet, each keeping today's synchronous lowering: a body
//! declaring a function or a class (the hoist's capture snapshot does
//! not reach a nested declaration, and a class lifted as a generator
//! field registers twice — rotation 641's two red shapes,
//! `declares_fn_or_class`); a parameter the parser synthesized
//! (destructuring prefix — the hoist re-registers that count for
//! declared generators only); and everything the decl gate refuses
//! (`desugar_async_sm_gate`).

use super::desugar_async_sm::{
    build_drive_call_to, generator_params, recv_generator_params, recv_wrapper_body, rewrite_body,
    wrapper_params,
};
use super::desugar_async_sm_gate::{blocks_move, declares_fn_or_class};
use super::desugar_async_sm_this::{reads_this, rewrite_this};
use super::{Ast, Expr, ExprId, GenFnExprInfo, GenFnExprKind, Stmt};

/// The moved value's receiver parameter (module doc: why not
/// `__genrecv`).
const VALUE_RECV_PARAM: &str = "__asyncrecv";

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
        if ast.async_ssa.value_exprs.contains(&eid)
            || params.iter().any(|p| p.name.starts_with("__"))
            || blocks_move(ast, &params, &body)
            || declares_fn_or_class(ast, &body)
        {
            continue;
        }
        let mut moved = body.clone();
        if !rewrite_body(ast, &mut moved) {
            continue;
        }
        let takes_recv = reads_this(ast, &moved);
        let gen_params = if takes_recv {
            rewrite_this(ast, &moved, VALUE_RECV_PARAM);
            recv_generator_params(&params, VALUE_RECV_PARAM)
        } else {
            generator_params(&params)
        };
        let generator = ast.add_expr(Expr::ArrowFn {
            params: gen_params,
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
        let body = if takes_recv {
            let this_eid = ast.add_expr(Expr::This);
            recv_wrapper_body(ast, generator, &params, this_eid)
        } else {
            let drive = build_drive_call_to(ast, generator, &params);
            vec![Stmt::Return(Some(drive))]
        };
        let wrapper_params = wrapper_params(ast, &params);
        ast.exprs[eid.0 as usize] = Expr::ArrowFn {
            params: wrapper_params,
            return_type: Some("Promise<any>".into()),
            body,
        };
        // The mark STAYS in `async_fn_value_exprs` (the lifted closure's
        // %AsyncFunction% flavour, the §15.8.4 param-throw posture);
        // only `desugar_async`'s value lane consults this set.
        ast.async_sm_value_exprs.insert(eid);
        moved_any = true;
    }
    moved_any
}
