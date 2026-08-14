//! Forwarder-decl synthesis — the "what does one `__forward_<t>`
//! look like" half of [`super::forwarders_object`], split to a
//! sibling when the 402-01 generic-target erasure pushed that file
//! past the 500-line limit. The parent keeps the store-site
//! collection / rewrite walk; this file builds the decls it asked
//! for.

use super::{Ast, Expr, ExprId, Param, Stmt};

/// 402-01 — substitute a generic target's own TypeVars with `any`
/// across the forwarder's param / return annotations (the target's
/// `type_params` are read off its FnDecl in place, so every caller
/// of the synthesizer gets the erasure without threading a table).
/// Non-generic targets pass through untouched.
fn erase_generic_typevars(
    ast: &Ast,
    target: &str,
    params: Vec<Param>,
    return_type: Option<String>,
) -> (Vec<Param>, Option<String>) {
    let type_params: Vec<String> = ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name, type_params, ..
            } if name == target => Some(type_params.clone()),
            _ => None,
        })
        .unwrap_or_default();
    if type_params.is_empty() {
        return (params, return_type);
    }
    let subst: Vec<(String, String)> = type_params
        .into_iter()
        .map(|tp| (tp, "any".to_string()))
        .collect();
    let params = params
        .into_iter()
        .map(|mut p| {
            if let Some(a) = &mut p.type_ann {
                *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, &subst);
            }
            p
        })
        .collect();
    let return_type =
        return_type.map(|rt| crate::ssa_lower_generics_monomorph::substitute_in_ann(&rt, &subst));
    (params, return_type)
}

/// Synthesize one `__forward_<target>(__env, args...) { return
/// target(args...); }` closure-shaped FnDecl per unique target
/// (skipping any `synthesize_forwarders` already produced), returning
/// the decls plus the target → forwarder-name rename map (chunk 783
/// extraction — the member-assign axis pushed the caller past the
/// 200-line fn limit).
pub(super) fn synthesize_forwarder_decls(
    ast: &mut Ast,
    targets: &std::collections::HashSet<String>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    existing_forwarders: &std::collections::HashSet<String>,
) -> (Vec<Stmt>, std::collections::HashMap<String, String>) {
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for target in targets {
        let forward_name = format!("__forward_{target}");
        if existing_forwarders.contains(&forward_name) {
            renames.insert(target.clone(), forward_name);
            continue;
        }
        let (params, return_type, target_span) = fn_sigs.get(target).unwrap().clone();
        // 402-01 — a GENERIC target's signature still spells its own
        // TypeVars (`gid<T>(v: T): T`), but the forwarder is a plain
        // FnDecl with no type params to bind them — the checker
        // rejects it loud ("unknown type `T`"). Erase every TypeVar
        // to `any`: the forwarder body's `target(v)` call is then an
        // ordinary generic call site the checker infers at Any, and
        // the mono pass emits the `$$_any` specialization the call
        // retargets to.
        let (params, return_type) = erase_generic_typevars(ast, target, params, return_type);
        let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
        fwd_params.push(Param {
            name: "__env".into(),
            type_ann: Some("__env()".to_string()),
            default: None,
            is_rest: false,
        });
        // Skip a promoted target's hidden `__this` first param on the
        // public face; forward `undefined` into it (mirrors
        // forwarders.rs — plain-call receiver semantics). A recv-first
        // public face was tried (rotation 366) and REVERTED: stamping
        // FLAG_CLOSURE_RECV_FIRST on forwarders shifted every caller
        // that doesn't honor the flag — the gen-argv tail's
        // `[...arguments]` swallowed the receiver into the generator
        // argv (the async-gen-static family), and the strict-this /
        // reduce-callback lanes misaligned their first argument —
        // 128 t262 passes lost. Threading the receiver through a
        // forwarder needs an audited design over every boxed-entry
        // caller first (plan-state L3b).
        let takes_this = params.first().is_some_and(|p| p.name == "__this");
        let user_params = if takes_this {
            &params[1..]
        } else {
            &params[..]
        };
        // Knife 4d — a trailing GEN_ARGV_PARAM never enters the
        // forwarder's declared face; the call below feeds it
        // `[...arguments]` (see forwarders::split_gen_argv_tail).
        let (user_params, takes_gen_argv) = super::forwarders::split_gen_argv_tail(user_params);
        fwd_params.extend(user_params.iter().cloned());
        let mut arg_eids: Vec<ExprId> = Vec::with_capacity(params.len());
        if takes_this {
            arg_eids.push(ast.add_expr(Expr::Ident("undefined".into())));
        }
        for p in user_params {
            let id = ast.add_expr(Expr::Ident(p.name.clone()));
            // A rest param forwards as a SPREAD: `target(args)` would
            // get re-packed by apply_rest_args (runs after this pass)
            // into `target([args])` — the double-wrap the adapter-side
            // rest collection exposed. `target(...args)` rides that
            // pass's single-spread arm (Array.from — §10.4.2's fresh
            // CreateArrayFromList) instead.
            if p.is_rest {
                arg_eids.push(ast.add_expr(Expr::Spread { expr: id }));
            } else {
                arg_eids.push(id);
            }
        }
        if takes_gen_argv {
            super::forwarders::push_gen_argv_spread(ast, &mut arg_eids);
        }
        let callee_id = ast.add_expr(Expr::Ident(target.clone()));
        let call_id = ast.add_expr(Expr::Call {
            callee: callee_id,
            args: arg_eids,
        });
        let body = vec![Stmt::Return(Some(call_id))];
        new_decls.push(Stmt::FnDecl {
            name: forward_name.clone(),
            type_params: Vec::new(),
            params: fwd_params,
            return_type,
            body,
            is_generator: false,
            // B4 — carry the TARGET's source span (toString answers
            // the wrapped user fn's source).
            span: target_span,
        });
        renames.insert(target.clone(), forward_name);
    }
    (new_decls, renames)
}
