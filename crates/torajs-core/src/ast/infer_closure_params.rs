//! Backward inference of anonymous-closure param/return type annotations
//! from the call site that consumes them.
//!
//! Chunk 342 — extracted from ast.rs. The pass + its two helper walkers
//! (`collect_let_anns` for explicit let annotations, `collect_let_init_anns`
//! for literal-init shape inference) form one logical unit and lift
//! cleanly into a single sibling. ast.rs re-exports `infer_anonymous_closure_params`
//! so external callers (`crates/torajs-cli/src/{cmd_build,main,lsp}.rs`)
//! keep their existing `ast::infer_anonymous_closure_params` path.
//!
//! RFC 20260705 chunk 554 — two contextual-typing extensions:
//! - **chained receivers**: `b.reverse().sort((x, y) => x - y)` — the
//!   receiver walks through identity-preserving Array methods
//!   (reverse / toReversed / sort / toSorted / fill / copyWithin /
//!   with / slice / filter / concat) back to the root binding's
//!   annotation instead of preinferring `(any, any)`.
//! - **user-fn callee hints**: `apply((n) => n + 1, 41)` — a
//!   `__fn(P|..)->(R)`-annotated param at a closure arg position
//!   projects its spellings onto the lifted closure's params/ret.

use super::infer_closure_params_apply::apply_closure_ann_updates;
use super::infer_closure_params_helpers::{
    apply_stored_elem_arm, build_ann_table, collect_fn_decl_metadata, mapset_foreach_expected,
    seed_container_arg_hints, seed_let_ann_hints,
};
use super::infer_closure_params_hof::apply_hof_param_only_arm;
use super::infer_closure_typevars::{UserFnHintSinks, apply_user_fn_callee_hint};
use super::{Ast, Expr, ExprId};

mod replace_cb;
use replace_cb::seed_replace_cb_anns;
mod sole_bindings;
pub(super) use sole_bindings::SoleBindings;
/* Resolve a literal expression's type ann string.
 * - `Array(els)`      → infer `T[]` from els[0]'s shape (literal
 *                       receiver path — `[1,2,3].map(x => ...)`).
 *                       Empty literal can't infer an element type;
 *                       skipped. Only homogeneous-typed literals
 *                       matter here since the existing `T[]` infra
 *                       requires homogeneous elements.
 * - `String`          → "string"
 * - `Number`          → "number"
 * Anything more exotic falls through unchanged (caller relies on
 * an explicit annotation upstream). */
pub(super) fn infer_lit_ann(ast: &Ast, eid: ExprId) -> Option<String> {
    match ast.get_expr(eid) {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::Array(els) if !els.is_empty() => {
            /* Recurse on EVERY element — the `T[]` claim is only
             * sound for a homogeneous literal (the header comment
             * always said so, but the code sampled els[0] only, so
             * `[1, Symbol()].forEach(props => ...)` typed the
             * callback param "number" and the Any elem coerce threw
             * ToNumber(Symbol) at the use site — rotation 162
             * staging appeared case). A mixed or unrecognized
             * literal answers None and the callback params stay
             * `any`. */
            let first = infer_lit_ann(ast, els[0])?;
            for &e in &els[1..] {
                if infer_lit_ann(ast, e).as_deref() != Some(first.as_str()) {
                    return None;
                }
            }
            Some(format!("{first}[]"))
        }
        _ => None,
    }
}

/// Resolve a receiver expression's type-annotation string.
/// - `Ident(n)` — the all_anns table (params + let anns + literal inits).
/// - `Call { callee: Member(inner, m) }` where `m` is an
///   identity-preserving Array method (answers the receiver's own
///   element type) — recurse on `inner`, so chained receivers
///   (`b.reverse().sort(cmp)`, RFC 20260705 chunk 554) reach the root
///   binding's annotation instead of preinferring `(any, any)`.
/// - `Member(inner, f)` — the declared type of field `f` on whatever
///   `inner` resolves to, nesting as deep as the path goes. A field
///   holds a container the same way a binding does, and its methods
///   are reached the same way: `o.fs.push(cb)` had no receiver type
///   at all, so the arrow it stores kept the parameter it gets with
///   no context and `o.fs[0](3)` answered -562949953421311.
/// - literal shapes — [`infer_lit_ann`].
///
/// `this_ann`, when given, is what `__this` names at this site, which
/// the flat table cannot answer (see
/// [`super::infer_closure_this_scope::this_anns_by_call`]).
pub(super) fn resolve_receiver_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
    this_ann: Option<&str>,
) -> Option<String> {
    const IDENTITY_PRESERVING: &[&str] = &[
        "reverse",
        "toReversed",
        "sort",
        "toSorted",
        "fill",
        "copyWithin",
        "with",
        "slice",
        "filter",
        "concat",
    ];
    match ast.get_expr(eid) {
        Expr::Ident(n) if n == "__this" && this_ann.is_some() => this_ann.map(str::to_string),
        Expr::Ident(n) => all_anns.get(n).cloned(),
        Expr::Member { obj, name } => {
            let obj_ann = resolve_receiver_ann(ast, *obj, all_anns, this_ann)?;
            super::infer_closure_lets::field_ann_of(ast, &obj_ann, name)
        }
        Expr::Call { callee, .. } => {
            let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                return None;
            };
            if IDENTITY_PRESERVING.contains(&name.as_str()) {
                resolve_receiver_ann(ast, *obj, all_anns, this_ann)
            } else {
                None
            }
        }
        _ => infer_lit_ann(ast, eid),
    }
}

/// The two kinds of argument a call site can type: the lifted closures
/// among its arguments, paired with their positions, and whether any
/// argument is a container literal.
///
/// A closure argument arrives in either post-lift shape —
/// `Expr::Closure { fn_name }` when the arrow captured something, a
/// bare ident at the lifted FnDecl when it did not — and both have to
/// be probed. A container literal is interesting for the same reason a
/// bare arrow is: the arrows inside it take the type the position
/// declares.
fn classify_call_args(ast: &Ast, args: &[ExprId]) -> (Vec<(usize, String)>, bool) {
    let mut closure_args: Vec<(usize, String)> = Vec::new();
    let mut has_container_arg = false;
    for (i, a) in args.iter().enumerate() {
        if let Some(fn_name) = super::infer_closure_lets::lifted_closure_name(ast, *a) {
            closure_args.push((i, fn_name));
        } else if matches!(ast.get_expr(*a), Expr::Array(_) | Expr::ObjectLit { .. }) {
            has_container_arg = true;
        }
    }
    (closure_args, has_container_arg)
}

/// Backward-infer the param type annotations of anonymous arrow
/// closures from the call site that consumes them. Runs after
/// `lift_arrow_fns` so each arrow is now a top-level FnDecl named
/// `__closure_<N>`; un-annotated params would later trip
/// `build_fn_type` with "parameter `a` requires a type annotation".
///
/// Inference rules (narrow MVP):
///   - Look for `Expr::Call { callee = Member(obj, method), args }`.
///   - For each arg that is an `Expr::Closure { fn_name }` with a
///     lifted FnDecl whose params lack annotations, look up the
///     receiver's type via the surrounding fn's let-decls + params.
///   - If the receiver type is `T[]` and the method is one of the
///     known callback-bearing Array methods (`sort` / `map` /
///     `filter` / `reduce` / `forEach` / `find` / `findIndex` /
///     `findLast` / `findLastIndex` / `some` / `every` / `flatMap`),
///     write the inferred per-position type annotations into the
///     lifted FnDecl.
///
/// Anything outside this rule (callbacks on non-Array receivers,
/// callbacks on un-annotated locals, etc.) keeps requiring explicit
/// type annotations.
pub fn infer_anonymous_closure_params(ast: &mut Ast) {
    use std::collections::HashMap;

    let (all_anns, closure_let_hints) = build_ann_table(ast);

    // Map from lifted closure fn_name → (param annotations, return
    // annotation). Filled by walking call sites; applied at the end
    // (deferred so we don't mutate ast.stmts mid-walk).
    let mut updates: HashMap<String, (Vec<String>, String)> = HashMap::new();

    // `const g: (n: number) => number = (n) => n` — the binding's target
    // type contextually types the arrow, exactly as at a call-arg
    // position (chunk-554's user-fn callee hint, on a let instead).
    // Seeded first so a call-site hint, which knows more about the
    // actual arguments, can still overwrite it.
    seed_let_ann_hints(ast, &closure_let_hints, &mut updates);

    // Promise `.then(cb)` / `.catch(cb)` arm — updates only the cb's
    // user params from the source promise inner type. Ret ann stays
    // for the sniff / desugar_lifted_closure_fn fallback (avoids
    // clobbering the cb's actual return face with a promise-inner
    // guess that only fits the param position).
    let mut param_only_updates: HashMap<String, Vec<String>> = HashMap::new();

    // srcArray-slot view promotion — lifted fn name → (user-slot
    // index, receiver elem ann); a receiver-matching user annotation
    // on that slot is normalized to `any[]` at apply time (rationale
    // in `apply_closure_ann_updates`).
    let mut view_promotes: HashMap<String, (usize, String)> = HashMap::new();

    let (fn_param_pos_anns, fn_type_params, fn_user_param_count, fn_ret_anns) =
        collect_fn_decl_metadata(ast);
    let param_callee_anns: HashMap<ExprId, Vec<Option<String>>> =
        crate::ast_closure_param_tag_axes::param_callee_calls(ast)
            .into_iter()
            .map(|(_, call, segs)| (call, segs.into_iter().map(Some).collect()))
            .collect();

    // walked on the first site that asks, not for every program
    let mut sole: Option<SoleBindings> = None;
    let this_anns = super::infer_closure_this_scope::this_anns_by_call(ast);
    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let args = args.clone();
        let (closure_args, has_container_arg) = classify_call_args(ast, &args);
        if closure_args.is_empty() && !has_container_arg {
            continue;
        }
        // User-fn callee: the callee's fn-typed param at a closure arg
        // position hints the lifted closure (`apply_user_fn_callee_hint`).
        if let Expr::Ident(fname) = ast.get_expr(callee) {
            // a fn-typed param of the function the call sits in shadows
            // any top-level function of that name
            let (own, no_tps);
            let (pos_anns, tps) = match param_callee_anns.get(&ExprId(i as u32)) {
                Some(anns) => {
                    own = HashMap::from([(fname.clone(), anns.clone())]);
                    no_tps = HashMap::new();
                    (&own, &no_tps)
                }
                None => (&fn_param_pos_anns, &fn_type_params),
            };
            if let Some(callee_anns) = pos_anns.get(fname) {
                apply_user_fn_callee_hint(
                    ast,
                    ExprId(i as u32),
                    &args,
                    &closure_args,
                    callee_anns,
                    tps.get(fname).map(Vec::as_slice),
                    UserFnHintSinks {
                        updates: &mut updates,
                        param_only: &mut param_only_updates,
                    },
                );
            }
            continue;
        }
        // Member(obj, method) — the Array-method path.
        let Expr::Member { obj, name } = ast.get_expr(callee).clone() else {
            continue;
        };
        if matches!(name.as_str(), "then" | "catch") {
            let promise_tables = super::infer_closure_params_promise::PromiseTables {
                all_anns: &all_anns,
                fn_ret_anns: &fn_ret_anns,
                bindings: sole.get_or_insert_with(|| SoleBindings::collect(ast)),
            };
            super::infer_closure_params_promise::seed_then_catch_params(
                ast,
                obj,
                &name,
                &promise_tables,
                &closure_args,
                &fn_user_param_count,
                &mut param_only_updates,
            );
            continue;
        }
        let this_ann = this_anns.get(&ExprId(i as u32)).map(String::as_str);
        let obj_ann = resolve_receiver_ann(ast, obj, &all_anns, this_ann);
        let Some(ann) = obj_ann else { continue };
        if ann == "string" && matches!(name.as_str(), "replace" | "replaceAll") {
            seed_replace_cb_anns(
                ast,
                &args,
                &closure_args,
                &fn_user_param_count,
                &mut updates,
            );
            continue;
        }
        // Map<K, V> / Set<T> receivers — forEach is the only
        // callback-bearing method (§23.1.3.5 / §24.2.3.6); cb
        // positional types are (V, K, Map) / (T, T, Set).
        if let Some(expected) = mapset_foreach_expected(&ann, &name) {
            for (_arg_idx, fn_name) in &closure_args {
                updates.insert(fn_name.clone(), expected.clone());
            }
            continue;
        }
        // `xs.concat([cb])` — §23.1.3.1 takes containers of elements,
        // not elements, so the literal takes the receiver's own type
        // rather than its element type.
        if name == "concat" {
            for a in &args {
                seed_container_arg_hints(ast, &ann, *a, &mut updates);
            }
            continue;
        }
        // An arrow at an element position of `push` / `unshift` /
        // `fill` / `with` / `splice` is stored, not called — the
        // receiver's element type is its type, the same contextual
        // typing a literal element already gets, in either spelling of
        // the array type.
        if let Some(elem_ann) = super::infer_closure_lets::array_elem_ann(&ann)
            && apply_stored_elem_arm(ast, &name, elem_ann, &closure_args, &mut updates)
        {
            continue;
        }
        // Only handle T[] receivers for the known Array methods. An
        // `Array<T>` receiver's callbacks keep their `any` params: a
        // typed callback calls the elements through the declared
        // element type, and an element stored from a function value of
        // another face (`any` return) is not reabstracted on the way
        // in, so the typed call would read it wrong.
        let Some(elem_ann) = ann.strip_suffix("[]") else {
            continue;
        };
        let elem_ann = elem_ann.to_string();
        if apply_hof_param_only_arm(
            ast,
            &name,
            &args,
            &closure_args,
            &elem_ann,
            &all_anns,
            &fn_user_param_count,
            &mut param_only_updates,
            &mut view_promotes,
        ) {
            continue;
        }
        let expected = super::infer_closure_params_helpers::hof_expected_anns(&name, &elem_ann);
        let Some(expected) = expected else { continue };
        for (_arg_idx, fn_name) in &closure_args {
            updates.insert(fn_name.clone(), expected.clone());
            if !matches!(name.as_str(), "sort" | "toSorted") {
                view_promotes.insert(fn_name.clone(), (2, elem_ann.clone()));
            }
        }
    }

    apply_closure_ann_updates(ast, &updates, &param_only_updates, &view_promotes);
}

// Promise `.then(cb)` / `.catch(cb)` cb-param inference lives in
// the sibling [`crate::ast::infer_closure_params_promise`] module
// (file-size hard limit — the main call-site loop plus the promise
// helpers didn't fit in one file).

// `mapset_foreach_expected` + `build_ann_table` moved to
// [`crate::ast::infer_closure_params_helpers`] (rotation-196 sweep);
// `apply_closure_ann_updates` + `body_returns_value` live in the
// sibling [`crate::ast::infer_closure_params_apply`] module — the
// mutation half was extracted so the main call-site walker stays
// within the file-size hard limit.
