//! Default-arg + rest-arg call-site desugar passes.
//!
//! Chunk 351 — extracted from ast.rs. Two independent passes that
//! together handle the call-site side of parameter-list features
//! (`f(x = expr)` defaults and `function f(...rest)` rest-args):
//!
//!   - `apply_default_args` — pads every `Expr::Call` to match its
//!     callee's default-value list; also handles the sibling-shape
//!     `obj.method(args)` case by grouping `__cm_<C>__<method>`
//!     FnDecls by method name and applying the common defaults when
//!     every owner agrees.
//!
//!   - `apply_rest_args` — packs trailing call-site args into an
//!     Array literal at the rest-param position; also emits per-type
//!     `__empty_arr__<sanitized>` helper FnDecls so the empty-rest
//!     shape lowers through a typed local.
//!
//! Both are `pub` because torajs-cli main / cmd_build / lsp / repl
//! call them at `torajs_core::ast::apply_default_args` etc; the
//! `pub use` in ast.rs preserves that path.

use super::{Ast, Expr, ExprId, Param, Stmt};
use std::collections::HashMap;

pub fn apply_default_args(ast: &mut Ast) {
    let mut fn_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
            if user_params.iter().any(|p| p.default.is_some()) {
                fn_defaults.insert(
                    name.clone(),
                    user_params.iter().map(|p| p.default).collect(),
                );
            }
        }
    }
    // Sibling-shape Member calls (`obj.method(args)`) survive desugar
    // when the method name is shared by unrelated classes (I.1). For
    // those, look up class-method FnDecls named `__cm_<C>__<method>`
    // and group by `<method>`. If every owner of `<method>` has the
    // same defaults shape (length + which positions have defaults),
    // we can apply them to the bare `obj.method(args)` call site
    // without knowing the receiver's static type.
    let mut method_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    let mut method_conflict: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (fname, defaults) in &fn_defaults {
        let Some(rest) = fname.strip_prefix("__cm_") else {
            continue;
        };
        // Use rfind: the class name itself may contain `__` (e.g.
        // `__Gen_count3`), so the method-name boundary is the LAST `__`.
        let Some(idx) = rest.rfind("__") else {
            continue;
        };
        let mname = &rest[idx + 2..];
        // The first param of __cm_C__M is __this (no default). Skip it
        // when comparing — the Member-call path doesn't pass __this
        // explicitly.
        if defaults.is_empty() {
            continue;
        }
        let user_defaults: Vec<Option<ExprId>> = defaults[1..].to_vec();
        if !user_defaults.iter().any(|d| d.is_some()) {
            continue;
        }
        if method_conflict.contains(mname) {
            continue;
        }
        match method_defaults.get(mname) {
            None => {
                method_defaults.insert(mname.to_string(), user_defaults);
            }
            Some(existing) => {
                // Conflict only if defaults shape differs (different
                // arity or different which-positions). We don't compare
                // ExprIds — different generator classes use different
                // ExprId for the same Number(0.0) literal but should
                // count as compatible. Compare lengths + Some/None
                // pattern only.
                let same_shape = existing.len() == user_defaults.len()
                    && existing
                        .iter()
                        .zip(&user_defaults)
                        .all(|(a, b)| a.is_some() == b.is_some());
                if !same_shape {
                    method_conflict.insert(mname.to_string());
                    method_defaults.remove(mname);
                }
            }
        }
    }

    // Alias map: `let f = __closure_N` (or `Closure{fn_name:__closure_N}`)
    // means a call to `f(...)` should adopt `__closure_N`'s defaults.
    // Without this, arrow fns with default params silently lose them at the
    // call site since the FnDecl is keyed by the synthetic closure name.
    let mut let_alias: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::LetDecl { name, init, .. } = s {
            let target = match ast.get_expr(*init) {
                Expr::Ident(n) if n.starts_with("__closure_") => Some(n.clone()),
                Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
                _ => None,
            };
            if let Some(t) = target {
                let_alias.insert(name.clone(), t);
            }
        }
    }

    if fn_defaults.is_empty() && method_defaults.is_empty() {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let args_len = args.len();
            // Pick defaults: prefer Ident match, fall back to Member
            // (sibling-shape) lookup.
            let defaults: Vec<Option<ExprId>> = match ast.get_expr(callee).clone() {
                Expr::Ident(name) => match fn_defaults.get(&name) {
                    Some(d) => d.clone(),
                    None => match let_alias.get(&name).and_then(|a| fn_defaults.get(a)) {
                        Some(d) => d.clone(),
                        None => continue,
                    },
                },
                Expr::Member { name, .. } => match method_defaults.get(&name) {
                    Some(d) => d.clone(),
                    None => continue,
                },
                _ => continue,
            };
            if args_len >= defaults.len() {
                continue;
            }
            let mut new_args = match &ast.exprs[i] {
                Expr::Call { args, .. } => args.clone(),
                _ => unreachable!(),
            };
            let mut ok = true;
            for j in args_len..defaults.len() {
                if let Some(default_eid) = defaults[j] {
                    new_args.push(default_eid);
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                ast.exprs[i] = Expr::Call {
                    callee,
                    args: new_args,
                };
            }
        }
    }
}

/// Pack trailing call-site args into an array literal when the
/// callee declares its last param with `...rest`. This pass mirrors
/// `apply_default_args` but for the rest-param shape.
///
/// The transformation: `f(a0, a1, …, ak)` where f's params are
/// `[p0, p1, ..., pn-1, ...rest]` becomes `f(a0, ..., an-1, [an, ..., ak])`
/// — the trailing args (positions n through k) get bundled into a
/// single Array literal at the rest-param position.
pub fn apply_rest_args(ast: &mut Ast) {
    // Map: callee name -> (n_required, rest_param_type_ann).
    let mut fn_rest: HashMap<String, (usize, String)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
            if let Some(last) = user_params.last() {
                if last.is_rest {
                    let n_required = user_params.len() - 1;
                    let rest_ann = last.type_ann.clone().unwrap_or_else(|| "any[]".into());
                    fn_rest.insert(name.clone(), (n_required, rest_ann));
                }
            }
        }
    }
    if fn_rest.is_empty() {
        return;
    }
    // Pre-synthesize empty-array helper FnDecls per rest type ann. Each
    // helper has shape `function __empty_arr_<sanitized>(): T[] {
    //   let _e: T[] = []; return _e; }`. The let-binding's annotation
    // gives ssa-lower the typed-empty-array path.
    let mut empty_helpers: HashMap<String, String> = HashMap::new();
    for (_, (_, rest_ann)) in &fn_rest {
        if !empty_helpers.contains_key(rest_ann) {
            let sanitized: String = rest_ann
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let helper_name = format!("__empty_arr__{sanitized}");
            empty_helpers.insert(rest_ann.clone(), helper_name);
        }
    }
    // Emit the helpers as new FnDecls.
    for (rest_ann, helper_name) in &empty_helpers {
        // Skip if already present.
        let exists = ast
            .stmts
            .iter()
            .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == helper_name));
        if exists {
            continue;
        }
        let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
        let body = vec![
            Stmt::LetDecl {
                mutable: false,
                name: "_e".into(),
                type_ann: Some(rest_ann.clone()),
                init: arr_lit,
                is_var: false,
            },
            Stmt::Return(Some(ast.add_expr(Expr::Ident("_e".into())))),
        ];
        ast.stmts.push(Stmt::FnDecl {
            name: helper_name.clone(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some(rest_ann.clone()),
            body,
            is_generator: false,
        });
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            let Some((n_required, rest_ann)) = fn_rest.get(&name).cloned() else {
                continue;
            };
            let args_clone = args.clone();
            if args_clone.len() < n_required {
                continue;
            }
            let mut new_args: Vec<ExprId> = args_clone[..n_required].to_vec();
            let rest_elems: Vec<ExprId> = args_clone[n_required..].to_vec();
            // Single-spread shape: `f(req…, ...arr)` — pass the spread
            // source array directly as the rest param. Common in
            // delegating wrappers.
            let single_spread_only =
                rest_elems.len() == 1 && matches!(ast.get_expr(rest_elems[0]), Expr::Spread { .. });
            let rest_arr = if rest_elems.is_empty() {
                let helper_name = empty_helpers.get(&rest_ann).cloned().unwrap();
                let callee_id = ast.add_expr(Expr::Ident(helper_name));
                ast.add_expr(Expr::Call {
                    callee: callee_id,
                    args: Vec::new(),
                })
            } else if single_spread_only {
                if let Expr::Spread { expr } = ast.get_expr(rest_elems[0]) {
                    *expr
                } else {
                    unreachable!()
                }
            } else {
                ast.add_expr(Expr::Array(rest_elems))
            };
            new_args.push(rest_arr);
            ast.exprs[i] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }
}
