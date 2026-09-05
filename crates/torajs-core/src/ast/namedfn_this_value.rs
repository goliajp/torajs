//! A `this`-using function declaration's VALUE is its forwarder.
//!
//! `bind_this_param` gives a top-level `function` whose body says
//! `this` a hidden leading `__this` parameter and seeds `undefined`
//! into every DIRECT call site. Its own doc records the other half as
//! a known boundary: "a call through a value (`const g = F; g(1)`)
//! cannot be reached from here and is left alone deliberately …
//! rather than papering over it with a silently wrong arity".
//!
//! The arity was silently wrong anyway, because nothing else closed
//! the boundary either. Measured on `040ec68bc`, all exit 0:
//!
//! ```text
//! function t(a: any) { return "a=" + a + "/" + typeof (this as any) }
//! const h = t;  h(7)              tr a=undefined/number   bun a=7/undefined
//! function take(f: (x: any) => string) { return f(7) }
//!               take(t)           tr a=true/number        bun a=7/undefined
//! const h = t;  h.length          tr 2                    bun 1
//! const h = t;  h === t           tr false                bun true
//! ```
//!
//! One cause: the value of such a declaration is sometimes the raw
//! `FnAddr` (arity including the receiver) and sometimes the
//! `__forward_<name>` shim (user-visible arity, receiver pinned to
//! `undefined`), decided by which earlier pass happened to claim the
//! site. The array-literal, object-literal and marked-argument routes
//! got the shim; a plain `const h = t` fell to
//! `ssa_lower_stmt_let_decl_global::try_lower_fn_addr_let`, whose
//! `FnSig` slot holds the raw address, and an argument reaching a slot
//! no fixpoint marked did the same.
//!
//! This pass makes the value ONE thing: every value use of a promoted
//! declaration becomes `Expr::Closure { __forward_<name> }`. Identity
//! then falls out of the `__fncell_*` singleton mint
//! (`ssa_lower_closure_canonical`), the arity is the forwarder's
//! declared face, and the receiver seed is the same `undefined` the
//! direct-call rewrite supplies.
//!
//! **Not** value uses, and deliberately untouched:
//!
//! - a `Call` callee — `t(7)` keeps the direct lane `bind_this_param`
//!   already rewrote, so the hot path is unchanged (B-4);
//! - a `Member` / `Index` base — `t.call(r, 7)` answers the real
//!   receiver today, and the forwarder pins `undefined`, so routing it
//!   here would trade a correct answer for a wrong one. The property
//!   face reads the same cell either way (`fnprops_bind_cell`);
//! - an `Assign` target — `t = …` is a write, and
//!   `widen_rebound_fn_decls` owns that shape.
//!
//! Declarations with no receiver parameter are not in
//! `this_param_fns` at all, so the plain named-fn direct lane
//! (`try_lower_fn_addr_let`) keeps every function that never mentions
//! `this` — which is the hot one.
//!
//! Pipeline position: immediately BEFORE
//! `tag_closure_arg_params`. The rewrite turns an argument site into
//! a closure cell, and the parameter it lands in has to be retagged to
//! the env-first repr by the same fixpoint run — a slot left spelled
//! `(x: any) => string` while the argument became a cell is a
//! calling-convention mismatch, and its symptom is a program that
//! prints nothing at all. Forwarder synthesizers that ran earlier have
//! already turned their sites into `Expr::Closure`, so this pass does
//! not see them; `synthesize_recv_cb_forwarders` runs LATER and wants
//! its sites still spelled as names, so it publishes them through
//! [`crate::ast::recv_cb_claimed_sites`] and this pass stands aside.
//! Those sites take the receiver-FIRST shim (`__fwdrecv_`), a
//! different cell from `__forward_` — so a declaration reached BOTH
//! ways still has two identities. `const h: any = t; h === t` is that
//! residue, and it is registered rather than papered over.

use std::collections::{HashMap, HashSet};

use super::{Ast, Expr, ExprId, Param, Stmt, push_gen_argv_spread, split_gen_argv_tail};

/// Rewrite value uses of `bind_this_param`-promoted declarations to
/// their forwarder, synthesizing the forwarder when no earlier pass
/// already did.
pub fn promote_this_fn_values(ast: &mut Ast) {
    if ast.this_param_fns.is_empty() {
        return;
    }
    let mut sigs: HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)> =
        HashMap::new();
    let mut have_forwarder: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            span,
            ..
        } = s
        else {
            continue;
        };
        have_forwarder.insert(name.clone());
        // The receiver parameter has to be THERE: a promoted name whose
        // declaration a later pass reshaped (a lift that gave it `__env`)
        // is no longer the shape this rewrite forwards to.
        if ast.this_param_fns.contains(name) && params.first().is_some_and(|p| p.name == "__this") {
            sigs.insert(name.clone(), (params.clone(), return_type.clone(), *span));
        }
    }
    if sigs.is_empty() {
        return;
    }

    // Every arena slot that holds an ExprId in a NON-value position.
    // The complement of this set, among `Ident`s naming a promoted
    // declaration, is exactly the value uses.
    let mut non_value: HashSet<usize> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Call { callee, .. } | Expr::NewDynamic { callee, .. } => {
                non_value.insert(callee.0 as usize);
            }
            Expr::Member { obj, .. } | Expr::Index { obj, .. } => {
                non_value.insert(obj.0 as usize);
            }
            Expr::Assign { target, .. } => {
                non_value.insert(target.0 as usize);
            }
            _ => {}
        }
    }
    let claimed = crate::ast::recv_cb_claimed_sites(ast);
    let mut sites: Vec<(usize, String)> = Vec::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        if let Expr::Ident(n) = e
            && sigs.contains_key(n)
            && !non_value.contains(&i)
            && !claimed.contains(&i)
        {
            sites.push((i, n.clone()));
        }
    }
    if sites.is_empty() {
        return;
    }

    let mut new_decls: Vec<Stmt> = Vec::new();
    for (_, target) in &sites {
        let forward_name = format!("__forward_{target}");
        if have_forwarder.contains(&forward_name) {
            continue;
        }
        have_forwarder.insert(forward_name.clone());
        let (params, return_type, target_span) = sigs.get(target).unwrap().clone();
        new_decls.push(synth_forwarder(
            ast,
            &forward_name,
            target,
            &params,
            return_type,
            target_span,
        ));
    }
    for (i, target) in sites {
        ast.exprs[i] = Expr::Closure {
            fn_name: format!("__forward_{target}"),
            captures: Vec::new(),
        };
    }
    ast.stmts.extend(new_decls);
}

/// `__forward_<target>(__env, ...user params) { return target(undefined, ...) }`
/// — the same shape `ast::forwarders::synthesize_forwarders` builds,
/// including the `__this` skip, the rest-param spread and the
/// generator-factory argv tail.
fn synth_forwarder(
    ast: &mut Ast,
    forward_name: &str,
    target: &str,
    params: &[Param],
    return_type: Option<String>,
    target_span: crate::lexer::Span,
) -> Stmt {
    let (user_params, takes_gen_argv) = split_gen_argv_tail(&params[1..]);
    let user_params = user_params.to_vec();
    let mut fwd_params: Vec<Param> = Vec::with_capacity(user_params.len() + 1);
    fwd_params.push(Param {
        name: "__env".into(),
        type_ann: Some("__env()".to_string()),
        default: None,
        is_rest: false,
    });
    fwd_params.extend(user_params.iter().cloned());
    let mut arg_eids: Vec<ExprId> = Vec::with_capacity(user_params.len() + 1);
    arg_eids.push(ast.add_expr(Expr::Ident("undefined".into())));
    for p in &user_params {
        let id = ast.add_expr(Expr::Ident(p.name.clone()));
        // A bare ident would be re-packed by `apply_rest_args` — the
        // sibling synthesizer spreads it for the same reason.
        if p.is_rest {
            arg_eids.push(ast.add_expr(Expr::Spread { expr: id }));
        } else {
            arg_eids.push(id);
        }
    }
    if takes_gen_argv {
        push_gen_argv_spread(ast, &mut arg_eids);
    }
    let callee_id = ast.add_expr(Expr::Ident(target.to_string()));
    let call_id = ast.add_expr(Expr::Call {
        callee: callee_id,
        args: arg_eids,
    });
    Stmt::FnDecl {
        name: forward_name.to_string(),
        type_params: Vec::new(),
        params: fwd_params,
        return_type,
        body: vec![Stmt::Return(Some(call_id))],
        is_generator: false,
        // Carry the TARGET's span — `toString` answers the wrapped
        // user function's source.
        span: target_span,
    }
}
