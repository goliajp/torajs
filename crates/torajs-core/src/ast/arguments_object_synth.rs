//! The synthesized `__torajs_arguments` local builders for
//! [`super::arguments_object::desugar_arguments_object`] — split out
//! of `arguments_object_walkers` (rotation 270) when the BareAssign
//! scan pushed that file past the 500-line limit; builders and
//! walkers were always two identities sharing one file (its own
//! header said so).

use super::{Ast, Expr, ExprId, Stmt};

/// T-11 — synthesize `let __torajs_arguments: any[] = [p0, p1, ...]`
/// for prepending to a fn body. Each param Ident becomes one array
/// element; the LetDecl arm in ssa_lower routes through the forced-
/// Any path because the annotation is `any[]`.
pub(super) fn synth_arguments_local(ast: &mut Ast, params: &[String]) -> Stmt {
    let elems: Vec<ExprId> = params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.clone())))
        .collect();
    let init = ast.add_expr(Expr::Array(elems));
    Stmt::LetDecl {
        mutable: false,
        name: "__torajs_arguments".into(),
        type_ann: Some("any[]".into()),
        init,
        is_var: false,
    }
}

/// Rest-param variant of [`synth_arguments_local`] — synthesize
/// `let __torajs_arguments: any[] = [f0, ..., fk, ...rest]`. The
/// trailing spread pulls the over-arity values out of the rest array
/// (apply_rest_args bundles them there downstream), so no positional
/// extras are injected for rest-tailed fns; `fixed` is already
/// sliced to `min(static argc, fixed params)` by the caller so an
/// under-filled site contributes only the args it actually passed.
pub(super) fn synth_arguments_local_rest(ast: &mut Ast, fixed: &[String], rest: &str) -> Stmt {
    let mut elems: Vec<ExprId> = fixed
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.clone())))
        .collect();
    let rest_ident = ast.add_expr(Expr::Ident(rest.into()));
    elems.push(ast.add_expr(Expr::Spread { expr: rest_ident }));
    let init = ast.add_expr(Expr::Array(elems));
    Stmt::LetDecl {
        mutable: false,
        name: "__torajs_arguments".into(),
        type_ann: Some("any[]".into()),
        init,
        is_var: false,
    }
}

/// RFC 20260708-closure-argv-face — synthesize
/// `let __torajs_arguments: any[] =
///   __torajs_arguments_materialize(__torajs_argv, __torajs_real_argc)`
/// for a full-arguments closure body. The synthetic call resolves in
/// the checker's ident special-case and lowers in the class-synth
/// lane to `arr_alloc_any` + `arr_any_push` over the adapter's argv,
/// so beyond-declared argument VALUES are reachable (the
/// `[p0, p1, …]` builder above only covers declared params).
pub(super) fn synth_arguments_local_argv(ast: &mut Ast) -> Stmt {
    let argv = ast.add_expr(Expr::Ident("__torajs_argv".into()));
    let argc = ast.add_expr(Expr::Ident("__torajs_real_argc".into()));
    let callee = ast.add_expr(Expr::Ident("__torajs_arguments_materialize".into()));
    let init = ast.add_expr(Expr::Call {
        callee,
        args: vec![argv, argc],
    });
    Stmt::LetDecl {
        mutable: false,
        name: "__torajs_arguments".into(),
        type_ann: Some("any[]".into()),
        init,
        is_var: false,
    }
}
