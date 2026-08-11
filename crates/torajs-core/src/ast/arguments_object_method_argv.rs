//! RFC 20260801-arguments-method-face knife 4a — runtime argv for
//! method VALUES. A class method reached only through member-value
//! reads (`var ref = C.prototype.m; ref(...)`, a getter returning
//! `this.#m`, `var ref = obj.m` after reification) is invoked through
//! the boxed dual-entry ABI `(env, argv, argc) -> AnyValue`, which
//! already delivers the true argc/argv to the `__boxed___cm_*`
//! adapter — where both are dropped today because the body declares
//! no slot for them. Admitting the body here injects the
//! `__torajs_argv` synthetic param (after
//! `__this`), and `build_boxed_entry` forwards the adapter's values
//! into them with no further change; the body then rides the
//! existing argv-face rewrite (materialize + Real-mode length).
//!
//! The admission is deliberately narrow — a signature reshape is a
//! memory-safety bug at any call site that doesn't know about it, so
//! every lane that could reach the fn with the OLD signature must be
//! provably absent:
//!
//! - **zero arena `Ident(name)` references** — a direct call
//!   (desugar's `__cm_C__m(recv, ...)` rewrite), a wrap-candidate
//!   argument, or a forwarder-relay body would each appear as an
//!   Ident; the static `CallIndirect` lane has no argc channel, so
//!   any Ident kills the admit. Member-value reads never mint an
//!   Ident (the S2.34 any-member-read arm boxes the receiver and
//!   resolves the method by NAME through the class-methods table).
//! - **not multi-owner / not on an inheritance chain** — the
//!   `__dispatch_<M>` and vtable lanes call by the old signature
//!   (matched by exact `__cm_<C>__<M>` construction over the known
//!   class names).
//! - **no default / rest / destructured params** — apply_default /
//!   apply_rest reshape the same param list; keep the two reshapes
//!   from composing (same defense as the static face).
//! - **≤ 5 user params** — `__this` + argc + argv + user must fit
//!   MAX_BOXED_PARAMS (8) or the adapter synthesis skips the fn and
//!   every dynamic call turns into a TypeError.
//!
//! Static methods (`__sm_*`) are NOT admitted: their value escape is
//! rewritten to a mint-Ident + `__forward_` relay (S2.37 axes), which
//! structurally truncates argc at the relay — widening the relay is
//! knife 4b. Generator methods join for free through their class-side
//! forwarder: it is an ordinary `__cm_` method whose tail arg is
//! `[...arguments]` (knife 2b), so once admitted the spread expands
//! the TRUE argv into the factory.

use super::arguments_object_walkers::{
    body_has_arguments_length, body_has_non_length_arguments_touch,
};
use super::{Ast, Expr, Stmt};

pub(super) fn collect_method_argv(
    ast: &Ast,
    excluded: &std::collections::HashSet<String>,
    static_argv: &std::collections::HashMap<String, usize>,
) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    // Exact `__cm_<C>__<M>` names owned by the multi-owner
    // (`__dispatch_`) and inheritance-chain (vtable) lanes — both
    // call by the OLD signature, so their implementations must not
    // reshape. Built exactly rather than by `__<M>` suffix matching:
    // a private method's mangled name (`__cm_A____priv_A__m`) ends
    // with `__m` too, and an unrelated sibling collision on `m`
    // must not evict it (the fixture caught exactly that).
    let mut class_names: std::collections::HashSet<&str> = ast
        .class_parents
        .keys()
        .map(|s| s.as_str())
        .chain(ast.class_parents.values().filter_map(|p| p.as_deref()))
        .collect();
    for owners in ast.method_owners.values() {
        for o in owners {
            class_names.insert(o.as_str());
        }
    }
    let mut old_sig_lane: std::collections::HashSet<String> = std::collections::HashSet::new();
    for m in ast.method_owners.keys().chain(ast.method_index.keys()) {
        for c in &class_names {
            old_sig_lane.insert(format!("__cm_{c}__{m}"));
        }
    }
    for s in &ast.stmts {
        let Stmt::FnDecl {
            name, params, body, ..
        } = s
        else {
            continue;
        };
        if !name.starts_with("__cm_") {
            continue;
        }
        // Generator factories (`__cm_gen_*`) carry `__genrecv` first,
        // not `__this` — the first-param test excludes them.
        if params.first().map(|p| p.name.as_str()) != Some("__this") {
            continue;
        }
        if excluded.contains(name) || static_argv.contains_key(name) {
            continue;
        }
        if !(body_has_arguments_length(ast, body) || body_has_non_length_arguments_touch(ast, body))
        {
            continue;
        }
        let user = &params[1..];
        if user.len() > 5 {
            continue;
        }
        if user
            .iter()
            .any(|p| p.default.is_some() || p.is_rest || p.name.starts_with("__param_destr_"))
        {
            continue;
        }
        if old_sig_lane.contains(name.as_str()) {
            continue;
        }
        if ast
            .exprs
            .iter()
            .any(|e| matches!(e, Expr::Ident(n) if n == name))
        {
            continue;
        }
        out.insert(name.clone());
    }
    out
}
