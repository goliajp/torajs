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

use super::{Ast, Param, Stmt, infer_return_ann};

/// Apply the deferred updates: mutate each lifted FnDecl's params +
/// return type (deferred so the call-site walk doesn't mutate
/// ast.stmts mid-walk).
pub(super) fn apply_closure_ann_updates(
    ast: &mut Ast,
    updates: &std::collections::HashMap<String, (Vec<String>, String)>,
    param_only_updates: &std::collections::HashMap<String, Vec<String>>,
    view_promotes: &std::collections::HashMap<String, (usize, String)>,
) {
    if updates.is_empty() && param_only_updates.is_empty() && view_promotes.is_empty() {
        return;
    }
    let (stmts, exprs, seeded) = (&mut ast.stmts, &ast.exprs, &mut ast.contextual_ret_fns);
    let implicit_rest = &ast.implicit_rest_fns;
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
            let rest_open = implicit_rest.contains(name.as_str());
            if let Some((new_param_anns, new_ret_ann)) = updates.get(name) {
                seed_user_params(&mut params[user_start..], new_param_anns, rest_open);
                // S2 (RFC 20260810-indirect-argc-abi) — a "void"
                // hint never seeds onto a value-returning body: TS's
                // void-return exception says the face IGNORES the
                // value, it doesn't forbid it. Seeding made the
                // body's own `return 7` a return-type mismatch; left
                // unseeded, the body infers its natural ret and the
                // call-site subtype admit (callback_subtype's Void
                // formal arm) accepts the pairing.
                if return_type.is_none() && body_returns_value(body) && new_ret_ann != "void" {
                    // rotation 285 — the predicate-family "boolean"
                    // seed is a FALLBACK, not a pin: ES ToBoolean
                    // folds any predicate ret, and the hetero-ret
                    // wedges admit non-Bool rets — but a seeded
                    // "boolean" made the body's own `return v % 2`
                    // a return-type mismatch before those wedges
                    // could look. Sniff the body's actual return
                    // first (params are already seeded above, so an
                    // elem-typed identity ret resolves). Rotation
                    // 651 — an unsniffable body seeds `any`, not the
                    // positional "boolean": `return undefined` /
                    // `return Symbol('')` / `return {}` are all
                    // ToBoolean-folded predicate answers (test262
                    // find/return-*-predicate-* × 8), and the seeded
                    // "boolean" made each a `check_stmt_return`
                    // mismatch before the call-site wedges could
                    // look. `any` passes the strict loop's callback-
                    // subtype widening and the predicate arms
                    // truth-test the box.
                    let ann = if new_ret_ann == "boolean" {
                        infer_return_ann(exprs, body, params, &std::collections::HashMap::new())
                            .unwrap_or_else(|| "any".to_string())
                    } else if let Some(folded) = new_ret_ann
                        .strip_prefix("__folded(")
                        .and_then(|r| r.strip_suffix(')'))
                    {
                        // A seed the SPEC converts rather than
                        // requires — sort's comparator result goes
                        // through ToNumber (§23.1.3.30). Pin it only
                        // when the body already produces that type,
                        // which keeps every numeric comparator on the
                        // typed lane it has always taken; anything
                        // else seeds `any`, where the kernel's own
                        // conversion is already correct and the
                        // callback-subtype rule admits an `Any` ret
                        // against the typed formal.
                        //
                        // Sniffing and pinning the sniffed type would
                        // not do: `return "1"` sniffs `string`, and a
                        // String ret is neither the formal nor `Any`,
                        // so it would be refused one step later.
                        match infer_return_ann(
                            exprs,
                            body,
                            params,
                            &std::collections::HashMap::new(),
                        ) {
                            Some(a) if a == folded => a,
                            _ => "any".to_string(),
                        }
                    } else {
                        new_ret_ann.clone()
                    };
                    *return_type = Some(ann);
                    seeded.insert(name.clone());
                }
            }
            if let Some(new_param_anns) = param_only_updates.get(name) {
                seed_user_params(&mut params[user_start..], new_param_anns, rest_open);
            }
            // srcArray-slot view promotion — a user-annotated
            // `{elem}[]` / `Array<{elem}>` on the spec srcArray slot
            // is normalized to `any[]` so the callback reads the
            // receiver block through the kind-aware Arr<Any> view
            // (a typed `number[]` view would raw-LoadDyn f64 over an
            // i64-slot literal block). Mismatched spellings are left
            // to loud-reject at the call site, matching tsc.
            if let Some((slot, elem)) = view_promotes.get(name)
                && let Some(p) = params.get_mut(user_start + slot)
                && p.type_ann
                    .as_deref()
                    .is_some_and(|t| t == format!("{elem}[]") || t == format!("Array<{elem}>"))
            {
                p.type_ann = Some("any[]".to_string());
            }
        }
    }
}

/// Seed a closure's user params from the target's param spellings,
/// position by position; a param the user annotated keeps its own.
///
/// A rest param with no annotation of its own (`rest_open`, the
/// `any[]` the parser gives it standing in) takes the target's rest
/// spelling at the same position: against `(...xs: number[]) => R`,
/// `(...xs) => …` has `xs: number[]` (TS's contextual typing of a rest
/// param). The target's `__rest(T[])` marker is the per-argument shape
/// of a fn TYPE; the closure's own param is the array itself, which is
/// how a declared rest param is spelled. A target with plain params at
/// that position would type the rest as a tuple, which has no spelling
/// here, so it keeps `any[]`.
fn seed_user_params(params: &mut [Param], anns: &[String], rest_open: bool) {
    let rest_arr = |a: &str| {
        a.strip_prefix("__rest(")
            .and_then(|r| r.strip_suffix(')'))
            .map(str::to_string)
    };
    // a trailing rest in the target covers every position from its own
    let tail = anns
        .last()
        .and_then(|a| rest_arr(a))
        .map(|arr| (anns.len() - 1, arr));
    for (i, p) in params.iter_mut().enumerate() {
        let (ann, from_rest) = match &tail {
            Some((at, arr)) if i >= *at => {
                if p.is_rest {
                    (arr.clone(), true)
                } else {
                    match crate::ast::infer_closure_lets::array_elem_ann(arr) {
                        Some(e) => (e.to_string(), false),
                        None => continue,
                    }
                }
            }
            _ => match anns.get(i) {
                Some(a) => (a.clone(), false),
                None => break,
            },
        };
        if p.type_ann.is_none() || (from_rest && rest_open) {
            p.type_ann = Some(ann);
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
