//! Default-arg call-site desugar pass.
//!
//! Chunk 351 — extracted from ast.rs. `apply_default_args` pads every
//! `Expr::Call` to match its callee's default-value list; it also
//! handles the sibling-shape `obj.method(args)` case by grouping
//! `__cm_<C>__<method>` FnDecls by method name and applying the common
//! defaults when every owner agrees.
//!
//! Its independent sibling `apply_rest_args` used to live here too and
//! now sits in `apply_rest_args.rs` — the two had grown past the
//! file-size line together. They share only `peel_hidden_params`,
//! which stays here.
//!
//! `pub` because torajs-cli main / cmd_build / lsp / repl call it at
//! `torajs_core::ast::apply_default_args`; the `pub use` in ast.rs
//! preserves that path.

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
pub(super) fn peel_hidden_params(params: &[Param]) -> &[Param] {
    // RFC 20260816-headless-argv-face — a head-less argv body leads
    // with the synthetic raw-argv pointer instead of a head param.
    // The direct-call terminal fills that slot from its own buffer,
    // so no call site counts it and neither may the default / rest
    // tables (leaving it in shifted every default one position and
    // made `f()` on `function f(a = 5)` refuse the call).
    if params.first().is_some_and(|p| p.name == "__torajs_argv") {
        return &params[1..];
    }
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

/// Method names some class row declares with a rest parameter — the
/// names the by-name default table must refuse to serve.
///
/// That table exists for a receiver this pass cannot resolve, and
/// what it would supply to a variadic row is not the omitted argument
/// the language owes (§10.2.11) but an EXTRA one: it lands in the
/// tail, so an unrelated class's `q = 3` reached a `f(x, ...r)` as
/// `r = [3]` and the row answered one more than it should. Seeded as
/// a conflict rather than filtered at the call site because a
/// conflict is what the name being ambiguous MEANS here; a receiver
/// this pass DOES resolve still reads the row's own defaults, one
/// branch earlier in `member_call_defaults`.
fn rest_tailed_method_names(ast: &Ast) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s
            && params.last().is_some_and(|p| p.is_rest)
            && let Some(rest) = name.strip_prefix("__cm_")
            && let Some(idx) = rest.rfind("__")
        {
            out.insert(rest[idx + 2..].to_string());
        }
    }
    out
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

/// One call's §10.2.11 default bindings: substitute a literal
/// `undefined` actual with the position's default (step 26 binds the
/// default for an explicit `undefined` exactly as for a missing
/// argument — `f(undefined)` on `f(x: number = 5)` is 5, and the
/// typed direct lane was rejecting it at the checker), then pad the
/// missing tail. A hole in the defaults past the actuals abandons the
/// padding (the language binds undefined there and the body-side lane
/// owns it) but keeps any substitutions. Runtime-undefined actuals
/// (an `any` value) are the lowering's or_default lane, not this
/// walk — a literal is the only spelling the AST can prove.
fn defaulted_args(ast: &Ast, args: &[ExprId], defaults: &[Option<ExprId>]) -> Option<Vec<ExprId>> {
    let mut new_args = args.to_vec();
    let mut subst = false;
    for (slot, dflt) in new_args.iter_mut().zip(defaults) {
        if let Some(default_eid) = dflt
            && matches!(ast.get_expr(*slot), Expr::Ident(n) if n == "undefined")
        {
            *slot = *default_eid;
            subst = true;
        }
    }
    let mut padded = args.len() < defaults.len();
    for dflt in &defaults[args.len().min(defaults.len())..] {
        match dflt {
            Some(default_eid) => new_args.push(*default_eid),
            None => {
                padded = false;
                new_args.truncate(args.len());
                break;
            }
        }
    }
    (subst || padded).then_some(new_args)
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
    let mut method_conflict = rest_tailed_method_names(ast);
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
    // Thin factories first — the walk below asks what a `let`'s
    // initializer constructs, and `const it = g()` can only be
    // answered once `g` is known to hand back a `__Gen_g`.
    let factories = super::apply_args_recv::collect_factory_classes(ast);
    super::apply_args_recv::collect_objlit_recv_fields(
        ast,
        &ast.stmts,
        &synthetic_fn_ident,
        &factories,
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
                        &factories,
                        &fn_defaults,
                        &method_defaults,
                    ) {
                        Some(d) => d,
                        None => continue,
                    }
                }
                _ => continue,
            };
            let args = match &ast.exprs[i] {
                Expr::Call { args, .. } => args.clone(),
                _ => unreachable!(),
            };
            if let Some(new_args) = defaulted_args(ast, &args, &defaults) {
                // Keep the source-written count: the hidden argc slot
                // the head-less tier fills reads the arg list, which
                // this rewrite is about to lengthen (see the field
                // doc on `Ast::default_padded_argc`).
                ast.default_padded_argc.insert(ExprId(i as u32), args.len());
                ast.exprs[i] = Expr::Call {
                    callee,
                    args: new_args,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fnd(name: &str, params: Vec<Param>) -> Stmt {
        Stmt::FnDecl {
            name: name.into(),
            type_params: Vec::new(),
            params,
            return_type: None,
            body: Vec::new(),
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        }
    }

    fn p(name: &str, is_rest: bool) -> Param {
        Param {
            name: name.into(),
            type_ann: None,
            default: None,
            is_rest,
        }
    }

    fn ast_of(stmts: Vec<Stmt>) -> Ast {
        Ast {
            stmts,
            ..Ast::default()
        }
    }

    #[test]
    fn a_variadic_row_refuses_the_name() {
        let ast = ast_of(vec![fnd(
            "__cm_A__f",
            vec![p("__this", false), p("x", false), p("r", true)],
        )]);
        assert!(rest_tailed_method_names(&ast).contains("f"));
    }

    #[test]
    fn a_fixed_row_leaves_the_name_alone() {
        let ast = ast_of(vec![fnd(
            "__cm_A__f",
            vec![p("__this", false), p("x", false), p("y", false)],
        )]);
        assert!(rest_tailed_method_names(&ast).is_empty());
    }

    #[test]
    fn the_method_boundary_is_the_last_double_underscore() {
        // a class name may itself contain `__` (`__Gen_count3`)
        let ast = ast_of(vec![fnd(
            "__cm___Gen_count3__next",
            vec![p("__this", false), p("r", true)],
        )]);
        let names = rest_tailed_method_names(&ast);
        assert!(names.contains("next"), "{names:?}");
    }

    #[test]
    fn a_plain_variadic_function_is_not_a_method_name() {
        let ast = ast_of(vec![fnd("g", vec![p("x", false), p("r", true)])]);
        assert!(rest_tailed_method_names(&ast).is_empty());
    }
}
