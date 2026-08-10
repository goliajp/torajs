//! The synthesized `__torajs_arguments` local builders for
//! [`super::arguments_object::desugar_arguments_object`] — split out
//! of `arguments_object_walkers` (rotation 270) when the BareAssign
//! scan pushed that file past the 500-line limit; builders and
//! walkers were always two identities sharing one file (its own
//! header said so).

use super::arguments_object::ArgcMode;
use super::{Ast, Expr, ExprId, Param, Stmt};

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
///   __torajs_arguments_materialize(__torajs_argv, <argc>)`
/// for a full-arguments closure body. The synthetic call resolves in
/// the checker's ident special-case and lowers in the class-synth
/// lane to `arr_alloc_any` + `arr_any_push` over the adapter's argv,
/// so beyond-declared argument VALUES are reachable (the
/// `[p0, p1, …]` builder above only covers declared params).
///
/// The take-count follows the face (RFC 20260810-indirect-argc-abi
/// S3.3): an env-first body reads the S1 hidden-ABI `__torajs_argc`;
/// a `__cm_` this-first body has no hidden slot and keeps the
/// injected `__torajs_real_argc` until the S1-extension blade covers
/// that entry family.
pub(super) fn synth_arguments_local_argv(ast: &mut Ast, env_first: bool) -> Stmt {
    let argv = ast.add_expr(Expr::Ident("__torajs_argv".into()));
    let argc_name = if env_first {
        "__torajs_argc"
    } else {
        "__torajs_real_argc"
    };
    let argc = ast.add_expr(Expr::Ident(argc_name.into()));
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

/// S3.2 write face — synthesize
/// `let __torajs_argc_len: number = __torajs_argc;` for an env-first
/// body that writes `arguments.length`: the S1 hidden argc is an
/// unwritable SSA param, so reads AND writes ride this mutable local
/// instead (the semantics the injected writable `__torajs_real_argc`
/// used to provide).
pub(super) fn synth_argc_len_local(ast: &mut Ast) -> Stmt {
    let init = ast.add_expr(Expr::Ident("__torajs_argc".into()));
    Stmt::LetDecl {
        mutable: true,
        name: "__torajs_argc_len".into(),
        type_ann: Some("number".into()),
        init,
        is_var: false,
    }
}

/// The `FLAG_ARR_ARGUMENTS` stamp statement —
/// `__torajs_arguments_mark(__torajs_arguments);` — inserted right
/// after the mint by [`super::arguments_object`]. The checker
/// resolves the callee ident as a synthetic (any) -> void; the
/// class-synth lowering lane expands it to the
/// `__torajs_arr_mark_arguments` runtime call, which sets the
/// Tag::Arr-private header bit the keyed `"length"` readers (gOPD /
/// delete / hasOwnProperty / member-get) gate on (§10.4.4 arguments
/// exotic length attributes).
pub(super) fn synth_arguments_mark(ast: &mut Ast) -> Stmt {
    let callee = ast.add_expr(Expr::Ident("__torajs_arguments_mark".into()));
    let arg = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
    let call = ast.add_expr(Expr::Call {
        callee,
        args: vec![arg],
    });
    Stmt::Expr(call)
}

/// RFC 20260810-sloppy-goal-arguments S2 — the sloppy goal's callee
/// seed, appended right after the mark on a materializing body:
///
/// ```text
/// Object.defineProperty(__torajs_arguments, "callee",
///   { value: <fn>, writable: true, enumerable: false,
///     configurable: true });
/// ```
///
/// §10.4.4 CreateMappedArgumentsObject step 21's ordinary data
/// property, landed through the ordinary defineProperty pipeline so
/// the expando-bag machinery (keyed read / write / delete / gOPD /
/// for-in filter) serves every later touch with zero new kernels.
/// The value expression follows the fn's [`CalleeSpelling`] — a
/// `Closure` literal of the `__forward_` shim, or a call of the
/// `__calleeval_` wrapper (the self-referential literal in the fn's
/// own body recursed the checker's walk).
pub(super) fn synth_sloppy_callee_define(
    ast: &mut Ast,
    spelling: &super::arguments_object_sloppy::CalleeSpelling,
) -> Stmt {
    let obj_ident = ast.add_expr(Expr::Ident("Object".into()));
    let dp = ast.add_expr(Expr::Member {
        obj: obj_ident,
        name: "defineProperty".into(),
    });
    let arr = ast.add_expr(Expr::Ident("__torajs_arguments".into()));
    let key = ast.add_expr(Expr::String("callee".into()));
    let value = match spelling {
        super::arguments_object_sloppy::CalleeSpelling::Shim(fwd) => ast.add_expr(Expr::Closure {
            fn_name: fwd.clone(),
            captures: Vec::new(),
        }),
        super::arguments_object_sloppy::CalleeSpelling::EvalCall(wrapper) => {
            let callee = ast.add_expr(Expr::Ident(wrapper.clone()));
            ast.add_expr(Expr::Call {
                callee,
                args: Vec::new(),
            })
        }
    };
    let t = ast.add_expr(Expr::Bool(true));
    let f = ast.add_expr(Expr::Bool(false));
    let t2 = ast.add_expr(Expr::Bool(true));
    let desc = ast.add_expr(Expr::ObjectLit {
        fields: vec![
            ("value".into(), value),
            ("writable".into(), t),
            ("enumerable".into(), f),
            ("configurable".into(), t2),
        ],
    });
    let call = ast.add_expr(Expr::Call {
        callee: dp,
        args: vec![arr, key, desc],
    });
    Stmt::Expr(call)
}

/// Build the materialized `__torajs_arguments` local for a fn under
/// a materializing mode. A rest-tailed fn spreads the rest array
/// (over-arity values live there — no extras were injected, see
/// inject_iife_static_params); everyone else takes the positional
/// builder over the leading `argc` params: over-arity is covered by
/// the injected extras already in `params`, an under-filled site
/// contributes only the args it actually passed.
pub(super) fn synth_materialized_arguments(
    ast: &mut Ast,
    decl_params: &[Param],
    params: &[String],
    argc_mode: ArgcMode,
) -> Stmt {
    if decl_params.last().is_some_and(|p| p.is_rest)
        && let Some((rest_name, fixed)) = params.split_last()
    {
        let take = match argc_mode {
            ArgcMode::FoldTo(n) | ArgcMode::LiveLength(n) | ArgcMode::Unmapped(n) => {
                n.min(fixed.len())
            }
            _ => fixed.len(),
        };
        synth_arguments_local_rest(ast, &fixed[..take], rest_name)
    } else {
        let take = match argc_mode {
            ArgcMode::FoldTo(n) | ArgcMode::LiveLength(n) | ArgcMode::Unmapped(n) => {
                n.min(params.len())
            }
            _ => params.len(),
        };
        synth_arguments_local(ast, &params[..take])
    }
}
