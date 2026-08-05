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

/// Peel a FnDecl's hidden leading params, leaving the user-facing ones.
///
/// A lifted closure leads with `__env`. An object-literal method (RFC
/// 20260714-objlit-accessor blade 1) leads with `__env, __this` — the
/// Member-call path supplies the receiver, so no call site counts it and
/// neither may the default / rest tables.
///
/// A CLASS method (`__cm_C__M`) leads with a bare `__this` and NO
/// `__env`, and its consumer below peels that itself (`defaults[1..]`).
/// So `__this` is only hidden here when it sits BEHIND `__env` —
/// stripping a leading one too would peel it twice and eat a real user
/// param ("expected 2 argument(s), got 1").
fn peel_hidden_params(params: &[Param]) -> &[Param] {
    if !params.first().is_some_and(|p| p.name == "__env") {
        return params;
    }
    let rest = &params[1..];
    if rest.first().is_some_and(|p| p.name == "__this") {
        return &rest[1..];
    }
    rest
}

/// Map every FnDecl with at least one defaulted param to its
/// user-param default list (hidden leading params peeled).
/// Shared between `apply_default_args` (call-site padding) and
/// `apply_spread_args` (defaulted-slot ternary expansion).
pub(crate) fn collect_fn_defaults(ast: &Ast) -> HashMap<String, Vec<Option<ExprId>>> {
    let mut fn_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let user_params: &[Param] = peel_hidden_params(params);
            if user_params.iter().any(|p| p.default.is_some()) {
                fn_defaults.insert(
                    name.clone(),
                    user_params.iter().map(|p| p.default).collect(),
                );
            }
        }
    }
    fn_defaults
}

/// Fold one method's default list into the Member-callee padding
/// table. Same-named methods from different owners stay compatible
/// only when their defaults agree — same arity, same defaulted
/// positions, and literal defaults with equal values (different
/// ExprIds for the same `Number(0.0)` literal count as equal; a
/// non-literal default never matches another owner's). A
/// disagreement evicts the name: padding a wrong default is a
/// silent-wrong, an unpadded call is an honest arity error.
fn merge_method_defaults(
    ast: &Ast,
    mname: &str,
    user_defaults: Vec<Option<ExprId>>,
    method_defaults: &mut HashMap<String, Vec<Option<ExprId>>>,
    method_conflict: &mut std::collections::HashSet<String>,
) {
    if !user_defaults.iter().any(|d| d.is_some()) {
        return;
    }
    if method_conflict.contains(mname) {
        return;
    }
    match method_defaults.get(mname) {
        None => {
            method_defaults.insert(mname.to_string(), user_defaults);
        }
        Some(existing) => {
            let compatible = existing.len() == user_defaults.len()
                && existing
                    .iter()
                    .zip(&user_defaults)
                    .all(|(a, b)| match (a, b) {
                        (None, None) => true,
                        (Some(x), Some(y)) => x == y || same_literal(ast, *x, *y),
                        _ => false,
                    });
            if !compatible {
                method_conflict.insert(mname.to_string());
                method_defaults.remove(mname);
            }
        }
    }
}

/// Literal-value equality across distinct ExprIds (the merge rule's
/// "same default" test). NaN literals compare equal to themselves.
fn same_literal(ast: &Ast, a: ExprId, b: ExprId) -> bool {
    match (ast.get_expr(a), ast.get_expr(b)) {
        (Expr::Number(x), Expr::Number(y)) => x == y || (x.is_nan() && y.is_nan()),
        (Expr::String(x), Expr::String(y)) => x == y,
        (Expr::Bool(x), Expr::Bool(y)) => x == y,
        (Expr::Null, Expr::Null) => true,
        // The materialize pass rewrites every expression default to
        // a fresh `undefined` literal per owner — same default.
        (Expr::Ident(x), Expr::Ident(y)) => x == "undefined" && y == "undefined",
        _ => false,
    }
}

/// Synthetic-fn classifier, shared by the let-alias walk and the
/// obj-literal method-field walk: `let f = __closure_N` (or a
/// `Closure{fn_name}` value) names the lifted FnDecl that carries
/// the defaults.
fn synthetic_fn_ident(e: &Expr) -> Option<String> {
    match e {
        Expr::Ident(n) if n.starts_with("__closure_") || n.starts_with("__genexpr_") => {
            Some(n.clone())
        }
        Expr::Closure { fn_name, .. } => Some(fn_name.clone()),
        _ => None,
    }
}

pub fn apply_default_args(ast: &mut Ast) {
    let fn_defaults = collect_fn_defaults(ast);
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
        merge_method_defaults(
            ast,
            mname,
            defaults[1..].to_vec(),
            &mut method_defaults,
            &mut method_conflict,
        );
    }
    // RFC 20260714-t262-top-clusters 刀 1a — obj-literal method
    // fields (`{ m(x = 5) {} }` lifts to a Closure-valued field)
    // join the Member-callee padding table by field name: `o.m()`
    // has no FnDecl-named call site, and the field-closure dispatch
    // never fires call-site defaults. Same merge rule as the
    // `__cm_` grouping above.
    for e in &ast.exprs {
        let Expr::ObjectLit { fields } = e else {
            continue;
        };
        for (fname, feid) in fields {
            let Some(target) = synthetic_fn_ident(ast.get_expr(*feid)) else {
                continue;
            };
            let Some(defaults) = fn_defaults.get(&target) else {
                continue;
            };
            merge_method_defaults(
                ast,
                fname,
                defaults.clone(),
                &mut method_defaults,
                &mut method_conflict,
            );
        }
    }

    // Alias map: `let f = __closure_N` (or `Closure{fn_name:__closure_N}`)
    // means a call to `f(...)` should adopt `__closure_N`'s defaults.
    // Without this, arrow fns with default params silently lose them at the
    // call site since the FnDecl is keyed by the synthetic closure name.
    // Hoisted generator expressions (`__genexpr_N`, RFC 20260713) join
    // via both binding shapes — `let f = <genexpr>` and the test262
    // staple `var f; f = function*(...) {...}` (top-level assign).
    let mut let_alias: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                if let Some(t) = synthetic_fn_ident(ast.get_expr(*init)) {
                    let_alias.insert(name.clone(), t);
                }
            }
            Stmt::Expr(eid) => {
                if let Expr::Assign { target, value } = ast.get_expr(*eid)
                    && let Expr::Ident(name) = ast.get_expr(*target)
                    && let Some(t) = synthetic_fn_ident(ast.get_expr(*value))
                {
                    let_alias.insert(name.clone(), t);
                }
            }
            _ => {}
        }
    }

    // Receiver-precise suppression for the Member arm below: the
    // name-keyed `method_defaults` table pads EVERY `x.m()` site, so
    // a receiver whose OWN `m` has no defaults was handed another
    // owner's default where the language gives undefined (probe:
    // `const b = {m(x: any){}}; b.m()` padded with an unrelated
    // `m(x = 5)`'s 5). When the receiver is an Ident bound to an
    // ObjectLit, resolve against the object's own field first; the
    // name-keyed table only serves unresolvable receivers.
    let mut binding_counts: HashMap<String, usize> = HashMap::new();
    let mut objlit_inits: HashMap<String, HashMap<String, Option<String>>> = HashMap::new();
    let mut class_inits: HashMap<String, String> = HashMap::new();
    super::apply_args_recv::collect_objlit_recv_fields(
        ast,
        &ast.stmts,
        &synthetic_fn_ident,
        &mut binding_counts,
        &mut objlit_inits,
        &mut class_inits,
    );
    // A name enters the map only when it is bound exactly once
    // program-wide, that binding is an ObjectLit, and it is never
    // reassigned — own-field resolution is provably sound there.
    // Everything else (multi-bound / reassigned / non-ObjectLit)
    // keeps the pre-existing name-keyed behavior: test262-style
    // programs rebind short names like `obj` constantly, and
    // suppressing those pads regressed a sweep by -30 (the callee
    // has no body-side default lane to fall back on).
    let mut recv_fields: HashMap<String, HashMap<String, Option<String>>> = HashMap::new();
    for (name, fields) in objlit_inits {
        if binding_counts.get(&name) == Some(&1) {
            recv_fields.insert(name, fields);
        }
    }
    // A class-instance receiver earns the same trust on the same
    // terms: bound exactly once program-wide and never reassigned.
    let mut recv_class: HashMap<String, String> = HashMap::new();
    for (name, class) in class_inits {
        if binding_counts.get(&name) == Some(&1) {
            recv_class.insert(name, class);
        }
    }
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && let Expr::Ident(nm) = ast.get_expr(*target)
        {
            recv_fields.remove(nm);
            recv_class.remove(nm);
        }
    }

    if fn_defaults.is_empty() && method_defaults.is_empty() {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            // A call carrying a spread arg has an unknown runtime arg
            // count — padding it would push the spread out of trailing
            // position and starve `apply_spread_args`, which handles
            // the defaulted slots itself (ternary length probes).
            if args
                .iter()
                .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
            {
                continue;
            }
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
                // Receiver-precise gate — see `member_call_defaults`
                // for the three sources and why the receiver's own
                // class outranks the name-keyed table.
                Expr::Member { obj, name } => {
                    match super::apply_args_recv::member_call_defaults(
                        ast,
                        obj,
                        &name,
                        &recv_fields,
                        &recv_class,
                        &fn_defaults,
                        &method_defaults,
                    ) {
                        Some(d) => d,
                        None => continue,
                    }
                }
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
            let user_params: &[Param] = peel_hidden_params(params);
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
            span: crate::lexer::Span { start: 0, end: 0 },
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
