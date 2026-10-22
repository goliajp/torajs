//! The executor arrow of `new Promise((resolve, reject) => …)`.
//!
//! `desugar_builtin_new` has already turned the expression into a call
//! of an executor helper, whose parameter is `any`, so the arrow
//! reached this pass with no context and both its parameters defaulted
//! to `any`. §27.2.3.1 fixes what they are: the two resolving functions
//! of §27.2.1.3, each called with one argument and returning undefined.
//!
//! `resolve` takes what TS spells `T | PromiseLike<T>`, `T` read from
//! the promise's context (`promise/context.rs`, the reading that also
//! chose the helper). This lattice has no `PromiseLike`, so a thenable
//! is admitted as an object with a `then` member. Where `T` is unknown,
//! `void`, or not spelled so that the union can be written around it,
//! `resolve` stays `any`, as `reject` always does. TS gives them
//! `(reason?: any) => void`, which this lattice cannot spell: a rest
//! `(...args: any[]) => void` refuses `let late: (e: number) => void;
//! late = reject`, and a one-parameter type refuses `reject()`, both
//! programs bun runs.

use super::super::{Ast, ExprId};
use super::SoleBindings;
use crate::ast_desugar_builtin_new::promise::context as promise_context;
use std::collections::HashMap;

/// Each executor-helper call with a known `T` → `T`; a `T` naming a
/// type parameter is out of scope for the lifted executor.
pub(super) fn executor_contexts(ast: &Ast, sole: &SoleBindings<'_>) -> HashMap<ExprId, String> {
    promise_context::executor_contexts(ast, |t| !sole.names_type_param(t, &[]))
}

/// Seed the executor arrow passed at argument 0 of the helper call
/// `call`.
pub(super) fn seed_executor_params(
    call: ExprId,
    closure_args: &[(usize, String)],
    contexts: &HashMap<ExprId, String>,
    fn_user_param_count: &HashMap<String, usize>,
    param_only: &mut HashMap<String, Vec<String>>,
) {
    for (arg_idx, fn_name) in closure_args {
        let p = fn_user_param_count.get(fn_name).copied().unwrap_or(0);
        if *arg_idx == 0 && p > 0 {
            let anns = executor_param_anns(contexts.get(&call).map(String::as_str), p);
            param_only.insert(fn_name.clone(), anns);
        }
    }
}

/// The executor's parameter annotations: `resolve`, then `any` for
/// `reject` and for any further parameter, which receives undefined.
fn executor_param_anns(inner: Option<&str>, user_params: usize) -> Vec<String> {
    let resolve = match inner {
        Some(t) if writable_in_union(t) => {
            // `(value: T | Promise<T> | { then: any }) => void`
            format!("__fn(__union({t}|Promise<{t}>|__inlobj(then:any)))->(void)")
        }
        _ => "any".to_string(),
    };
    let mut anns = vec!["any".to_string(); user_params];
    anns[0] = resolve;
    anns
}

/// Can `t` stand as one member of a union without parentheses, and does
/// it say more than "anything"?
fn writable_in_union(t: &str) -> bool {
    !matches!(t, "any" | "unknown" | "void" | "undefined")
        && !t.contains("__")
        && t.chars().all(|c| {
            c.is_alphanumeric() || matches!(c, '_' | '$' | ' ' | '<' | '>' | ',' | '[' | ']' | '|')
        })
}
