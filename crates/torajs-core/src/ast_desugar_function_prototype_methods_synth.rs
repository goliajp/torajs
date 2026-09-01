//! The `.bind` rewrite's synthesis half of
//! [`crate::ast_desugar_function_prototype_methods`] — split out
//! when RFC 20260808 knife 2's kernel-lane routing pushed the parent
//! past the 500-line cap. Verbatim move; the parent calls through
//! `synth_bind`.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};

/// The `.bind` rewrite's synthesis half: mint the `__bound_<f>_<id>`
/// wrapper + `__bind_create_<f>_<id>` factory pair and replace the
/// call at arena slot `i` with the factory invocation over the
/// partial args (see module doc).
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn synth_bind(
    ast: &mut Ast,
    i: usize,
    id: u32,
    fn_name: &str,
    fn_params: &[Param],
    fn_ret: &Option<String>,
    partial_args: Vec<ExprId>,
    new_decls: &mut Vec<Stmt>,
    // fnexpr-bind knife — Some(binding) when the receiver is a
    // closure BINDING rather than a named FnDecl: the wrapper cannot
    // spell the lifted `__closure_N` (its call would need the hidden
    // `__env`), so the target closure VALUE itself rides the bound
    // env — §10.4.1's [[BoundTargetFunction]], one to one — and the
    // wrapper calls through that capture.
    target_binding: Option<&str>,
) {
    let partial_count = partial_args.len();
    let bound_name = format!("__bound_{}_{}", fn_name, id);
    let factory_name = format!("__bind_create_{}_{}", fn_name, id);

    let cap_names: Vec<String> = (0..partial_count)
        .map(|k| format!("__bp_{}_{}", id, k))
        .collect();
    let remaining_count = fn_params.len() - partial_count;
    let rem_names: Vec<String> = (0..remaining_count)
        .map(|k| format!("__br_{}_{}", id, k))
        .collect();

    let target_cap = target_binding.map(|_| format!("__bt_{id}"));
    let env_names: Vec<String> = target_cap
        .iter()
        .cloned()
        .chain(cap_names.iter().cloned())
        .collect();
    let env_ann = format!("__env({})", env_names.join("|"));
    let mut bound_params: Vec<Param> = Vec::with_capacity(remaining_count + 1);
    bound_params.push(Param {
        name: "__env".into(),
        type_ann: Some(env_ann),
        default: None,
        is_rest: false,
    });
    for k in 0..remaining_count {
        let src_param = &fn_params[partial_count + k];
        bound_params.push(Param {
            name: rem_names[k].clone(),
            type_ann: src_param.type_ann.clone(),
            default: None,
            is_rest: false,
        });
    }
    let callee_ident_id = ast.add_expr(Expr::Ident(
        target_cap.clone().unwrap_or_else(|| fn_name.to_string()),
    ));
    let mut call_args: Vec<ExprId> = Vec::with_capacity(fn_params.len());
    for cn in &cap_names {
        call_args.push(ast.add_expr(Expr::Ident(cn.clone())));
    }
    for rn in &rem_names {
        call_args.push(ast.add_expr(Expr::Ident(rn.clone())));
    }
    let call_id = ast.add_expr(Expr::Call {
        callee: callee_ident_id,
        args: call_args,
    });
    let bound_body = vec![Stmt::Return(Some(call_id))];
    // An unannotated source fn's return type is inferred
    // later — `any` is the only annotation the factory can
    // write that agrees with whatever that inference says
    // (`void` rejected every value-returning unannotated fn).
    let ret_type_str = fn_ret.clone().unwrap_or_else(|| "any".to_string());
    let bound_decl = Stmt::FnDecl {
        name: bound_name.clone(),
        type_params: Vec::new(),
        params: bound_params,
        // Mirrors the factory's `->R` (any when the source
        // is unannotated) so the wrapper's own return
        // boundary boxes the forwarded value — an inferred
        // Number returned raw through a `__cls(..)->(any)`
        // slot reads back as a garbage tag.
        return_type: Some(ret_type_str.clone()),
        body: bound_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    };

    let mut factory_params: Vec<Param> = Vec::with_capacity(partial_count + 1);
    if let Some(tc) = &target_cap {
        let all_tys: Vec<String> = fn_params
            .iter()
            .map(|p| p.type_ann.clone().unwrap_or_else(|| "any".to_string()))
            .collect();
        factory_params.push(Param {
            name: tc.clone(),
            type_ann: Some(crate::type_ann_fnsig::fn_type_ann(
                "__cls",
                &all_tys.join("|"),
                &ret_type_str,
            )),
            default: None,
            is_rest: false,
        });
    }
    for k in 0..partial_count {
        let src_param = &fn_params[k];
        factory_params.push(Param {
            name: cap_names[k].clone(),
            type_ann: src_param.type_ann.clone(),
            default: None,
            is_rest: false,
        });
    }
    let rem_tys: Vec<String> = (partial_count..fn_params.len())
        .map(|k| {
            fn_params[k]
                .type_ann
                .clone()
                .unwrap_or_else(|| "any".to_string())
        })
        .collect();
    let factory_ret =
        crate::type_ann_fnsig::fn_type_ann("__cls", &rem_tys.join("|"), &ret_type_str);
    let closure_expr_id = ast.add_expr(Expr::Closure {
        fn_name: bound_name,
        captures: env_names.clone(),
    });
    let factory_body = vec![Stmt::Return(Some(closure_expr_id))];
    new_decls.push(Stmt::FnDecl {
        name: factory_name.clone(),
        type_params: Vec::new(),
        params: factory_params,
        return_type: Some(factory_ret),
        body: factory_body,
        is_generator: false,
        span: crate::lexer::Span { start: 0, end: 0 },
    });
    new_decls.push(bound_decl);

    let new_callee = ast.add_expr(Expr::Ident(factory_name));
    let mut factory_args = partial_args;
    if let Some(binding) = target_binding {
        let t = ast.add_expr(Expr::Ident(binding.to_string()));
        factory_args.insert(0, t);
    }
    ast.exprs[i] = Expr::Call {
        callee: new_callee,
        args: factory_args,
    };
}
