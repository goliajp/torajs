//! How a target's head params feed the adapter (split out of
//! `targets`).

/// Head-param classification of one FnDecl: `(first_is_env,
/// first_is_this, is_twin)`. Env-first (lifted closure) and
/// this-first (`__cm_` mono method) bodies feed the adapter's env
/// argument into their pointer-shaped head param. A `__cmany_`
/// twin's `__this` is an ANY param, not a pointer (blade 3, RFC
/// 20260804-method-rebind-generic-body): it must NOT ride the env
/// channel (the env ptr's raw bits are not a nanbox) — it maps as an
/// ordinary user param instead, and the guard-fail path calls the
/// adapter recv-first (receiver prepended in argv[0], any→any
/// pass-through). An argv-face twin (`__torajs_argv` right after
/// `__this`, the this-first argv lane) maps the same way, with the
/// receiver read off argv[0] ahead of the hidden slots and the
/// window shifted past it for the rest (`recv_leads`).
pub(super) fn head_shape(name: &str, params: &[crate::ast::Param]) -> (bool, bool, bool) {
    let head_is_this = params.first().is_some_and(|p| p.name == "__this");
    (
        params.first().is_some_and(|p| p.name == "__env"),
        head_is_this && name.starts_with("__cm_") && !name.starts_with("__cmany_"),
        head_is_this
            // Knife 3b (RFC 20260804-fn-this-channel) — the static
            // twin `__smany_` has the same any-typed `__this` head
            // and the same recv-first adapter shape. The ctor twin
            // `__ctorany_` (RFC 20260815 knife 2b) is the same shape
            // again: `(__this, __new_target, …user)`, every param
            // any — the value-parent super registry calls it through
            // this adapter.
            && (name.starts_with("__cmany_")
                || name.starts_with("__smany_")
                || name.starts_with("__ctorany_")),
    )
}
