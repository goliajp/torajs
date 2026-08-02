//! Deferred update application for [`crate::ast::infer_closure_params`]
//! — the mutation half of the pass, extracted into a sibling so the
//! main call-site walker stays within the file-size hard limit.
//!
//! Two independent update kinds are folded into one lifted-FnDecl
//! walk:
//! - `updates` — array-method + user-fn callee arms; may seed both
//!   the cb's user params AND its return annotation (guarded by
//!   `body_returns_value`, since a Void-body cb must not be
//!   promoted to a value-returning signature).
//! - `param_only_updates` — Promise `.then/.catch` arm; seeds only
//!   the cb's user params from the source promise inner. Ret ann
//!   stays for the up-front sniff / `desugar_lifted_closure_fn`
//!   fallback, so the cb's actual return face isn't clobbered by a
//!   promise-inner guess that only fits the param position.

use super::{Ast, Stmt, infer_return_ann};

/// Apply the deferred updates: mutate each lifted FnDecl's params +
/// return type (deferred so the call-site walk doesn't mutate
/// ast.stmts mid-walk).
pub(super) fn apply_closure_ann_updates(
    ast: &mut Ast,
    updates: &std::collections::HashMap<String, (Vec<String>, String)>,
    param_only_updates: &std::collections::HashMap<String, Vec<String>>,
) {
    if updates.is_empty() && param_only_updates.is_empty() {
        return;
    }
    let (stmts, exprs) = (&mut ast.stmts, &ast.exprs);
    for stmt in stmts.iter_mut() {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = stmt
        {
            // First param of a lifted closure is `__env`; user params
            // start at index 1.
            let user_start = if params.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            if let Some((new_param_anns, new_ret_ann)) = updates.get(name) {
                for (i, ann) in new_param_anns.iter().enumerate() {
                    let pidx = user_start + i;
                    if let Some(p) = params.get_mut(pidx)
                        && p.type_ann.is_none()
                    {
                        p.type_ann = Some(ann.clone());
                    }
                }
                if return_type.is_none() && body_returns_value(body) {
                    // rotation 285 — the predicate-family "boolean"
                    // seed is a FALLBACK, not a pin: ES ToBoolean
                    // folds any predicate ret, and the hetero-ret
                    // wedges admit non-Bool rets — but a seeded
                    // "boolean" made the body's own `return v % 2`
                    // a return-type mismatch before those wedges
                    // could look. Sniff the body's actual return
                    // first (params are already seeded above, so an
                    // elem-typed identity ret resolves); only an
                    // unsniffable body keeps the positional seed.
                    let ann = if new_ret_ann == "boolean" {
                        infer_return_ann(exprs, body, params, &std::collections::HashMap::new())
                            .unwrap_or_else(|| new_ret_ann.clone())
                    } else {
                        new_ret_ann.clone()
                    };
                    *return_type = Some(ann);
                }
            }
            if let Some(new_param_anns) = param_only_updates.get(name) {
                for (i, ann) in new_param_anns.iter().enumerate() {
                    let pidx = user_start + i;
                    if let Some(p) = params.get_mut(pidx)
                        && p.type_ann.is_none()
                    {
                        p.type_ann = Some(ann.clone());
                    }
                }
            }
        }
    }
}

/// True when any statement in `body` (recursing through control-flow
/// constructs) is a `return <expr>;`. A closure whose body never
/// produces a value must keep its `Void` return type — seeding a value
/// ret ann onto it makes the lowered fn fall off the end of a value-
/// returning signature, which codegen terminates with `unreachable`
/// (SIGTRAP at the first call). JS-wise such a callback returns
/// `undefined`; the Void-ret callback arms in check / ssa_lower carry
/// that semantic instead.
pub(super) fn body_returns_value(body: &[Stmt]) -> bool {
    body.iter().any(|s| match s {
        Stmt::Return(ret) => ret.is_some(),
        Stmt::Block(inner) | Stmt::Multi(inner) => body_returns_value(inner),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            body_returns_value(std::slice::from_ref(then_branch.as_ref()))
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| body_returns_value(std::slice::from_ref(eb.as_ref())))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::ForOfSplitIter { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { body, .. } => body_returns_value(std::slice::from_ref(body.as_ref())),
        Stmt::For { init, body, .. } => {
            init.as_ref()
                .is_some_and(|i| body_returns_value(std::slice::from_ref(i.as_ref())))
                || body_returns_value(std::slice::from_ref(body.as_ref()))
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| body_returns_value(&c.body))
                || default.as_ref().is_some_and(|d| body_returns_value(d))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body_returns_value(body)
                || body_returns_value(catch_body)
                || finally_body.as_ref().is_some_and(|f| body_returns_value(f))
        }
        _ => false,
    })
}
