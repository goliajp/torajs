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
    apply_stored_elem_arm, apply_user_fn_callee_hint, build_ann_table, collect_fn_decl_metadata,
    mapset_foreach_expected, seed_container_arg_hints, seed_let_ann_hints,
};
use super::infer_closure_params_hof::apply_hof_param_only_arm;
use super::infer_closure_params_promise::resolve_promise_inner_ann;
use super::{Ast, Expr, ExprId};
use crate::ssa_lower_free_helpers::count_capture_groups;

/// Annotations for a `<string>.replace(re, cb)` callback's user params,
/// shaped by the callback's user-param count `p` and the regex's static
/// capture count `n_caps` (`None` when the regex isn't a literal). The
/// match + captures are Strings; an optional trailing pair is
/// `(offset: number, input: string)`. A shape fitting neither accepted
/// lane form (`[Str; C+1]` or `[Str; C+1, number, string]`) annotates
/// only the match (param 0) — always a String — and leaves the rest for
/// the regex lane to loud-reject (never mis-typed).
fn replace_cb_param_anns(p: usize, n_caps: Option<usize>) -> Vec<String> {
    if let Some(c) = n_caps {
        if p == c + 1 {
            return vec!["string".to_string(); p];
        }
        if p == c + 3 {
            let mut v = vec!["string".to_string(); c + 1];
            v.push("number".to_string());
            v.push("string".to_string());
            return v;
        }
    }
    vec!["string".to_string()]
}

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
pub(super) fn resolve_receiver_ann(
    ast: &Ast,
    eid: ExprId,
    all_anns: &std::collections::HashMap<String, String>,
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
        Expr::Ident(n) => all_anns.get(n).cloned(),
        Expr::Member { obj, name } => {
            let obj_ann = resolve_receiver_ann(ast, *obj, all_anns)?;
            super::infer_closure_lets::field_ann_of(ast, &obj_ann, name)
        }
        Expr::Call { callee, .. } => {
            let Expr::Member { obj, name } = ast.get_expr(*callee) else {
                return None;
            };
            if IDENTITY_PRESERVING.contains(&name.as_str()) {
                resolve_receiver_ann(ast, *obj, all_anns)
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
        match ast.get_expr(*a) {
            Expr::Closure { fn_name, .. } => closure_args.push((i, fn_name.clone())),
            Expr::Ident(n) if n.starts_with("__closure_") => closure_args.push((i, n.clone())),
            Expr::Array(_) | Expr::ObjectLit { .. } => has_container_arg = true,
            _ => {}
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
        // User-fn callee: an `__fn(P|..)->(R)`-annotated param at a
        // closure arg position hints the lifted closure's own
        // param/ret annotations (chunk 554 face ②).
        //
        // Generic callee (chunk 682) — the hint spellings may mention
        // the callee's type params (`g<T>(cb: (...args: T[]) => T,
        // x: T)`); projecting a raw `T` onto the lifted closure trips
        // `build_fn_type` with "unknown return type `T`" (the typevar
        // is out of scope there). Resolve typevars from the same call
        // site first (a bare-typevar param position holding a literal
        // arg pins it: `x: T` + `21` → T=number); a spelling still
        // mentioning an unresolved typevar after substitution is not
        // projected.
        if let Expr::Ident(fname) = ast.get_expr(callee) {
            apply_user_fn_callee_hint(
                ast,
                fname,
                &args,
                &closure_args,
                &fn_param_pos_anns,
                &fn_type_params,
                &mut updates,
            );
            continue;
        }
        // Member(obj, method) — the Array-method path.
        let Expr::Member { obj, name } = ast.get_expr(callee).clone() else {
            continue;
        };
        // Promise<T>.then(cb) / .catch(cb) — infer cb's first param
        // from the source promise inner type (Promise.resolve/reject
        // of a literal, or a chained cb's return ann). Without this
        // the cb's un-annotated param would default to `any` in
        // `desugar_lifted_closure_fn`; the pthunk pre-scan then skips
        // it (its param-face gate accepts only `None | Some("number")`)
        // and the runtime dispatcher hands the raw i64 slot to a cb
        // whose Any param can't decode an F64 bit pattern — the user
        // sees the raw bits as a huge integer.
        if matches!(name.as_str(), "then" | "catch") {
            if let Some(inner_ann) = resolve_promise_inner_ann(ast, obj, &all_anns, &fn_ret_anns) {
                // A `T | null` source seeds `any`, not `T | null`.
                // This pass reads the DECLARATION — it runs long
                // before anything knows about narrowing — so
                // `s = "hi"; Promise.resolve(s).then((v) => v + "!")`
                // seeded `v: string | null` and the body was refused
                // on a program whose value is a string throughout.
                // The receiver, which does see the narrow, says
                // `Promise<string>`; `handler_param_admits` already
                // reconciles the two SIGNATURES, but the body is
                // checked against whatever this pass wrote.
                //
                // `any` is what a nullable rides anyway
                // (`rides_any_lane`), and it is the honest seed here:
                // this pass cannot know whether the value reaching
                // the handler is null. Unseeded it would default to
                // `any` too — the seed exists to keep an F64 source
                // off the raw-i64 slot, which a nullable source never
                // was. An un-narrowed null then reaches the body as a
                // null and fails there, which is where bun fails.
                // A handler that writes its own annotation keeps it
                // (the seed only fills an absent one).
                let seed = if inner_ann.starts_with("__nullable(") {
                    "any".to_string()
                } else {
                    inner_ann
                };
                for (_arg_idx, fn_name) in &closure_args {
                    let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
                    if p >= 1 {
                        param_only_updates.insert(fn_name.clone(), vec![seed.clone(); p]);
                    }
                }
            }
            continue;
        }
        let obj_ann = resolve_receiver_ann(ast, obj, &all_anns);
        let Some(ann) = obj_ann else { continue };
        // `<string>.replace(re, cb)` / `replaceAll(re, cb)` — the
        // replacer receives `(match, ...captures, offset, input)`
        // (§22.1.3.19 GetSubstitution). The regex-lane cb-sig check
        // accepts exactly `[Str; C+1] -> Str` (match + C captures) or
        // `[Str; C+1, number, string] -> Str` (…+ offset + input), so
        // shape the annotations by the literal's capture count `C`
        // (same counter the lane uses). A non-literal (variable) regex
        // has an unknown C — annotate only the match (user param 0),
        // which is always a String; the lane then accepts the common
        // `(m) => …` shape and loud-rejects anything else (never
        // mis-typed).
        if ann == "string" && matches!(name.as_str(), "replace" | "replaceAll") {
            let n_caps = match args.first().map(|a| ast.get_expr(*a)) {
                Some(Expr::Regex { pattern, .. }) => Some(count_capture_groups(pattern)),
                _ => None,
            };
            for (_arg_idx, fn_name) in &closure_args {
                let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
                let param_anns = replace_cb_param_anns(p, n_caps);
                updates.insert(fn_name.clone(), (param_anns, "string".into()));
            }
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
        // Only handle T[] receivers for the known Array methods.
        let Some(elem_ann) = ann.strip_suffix("[]") else {
            continue;
        };
        let elem_ann = elem_ann.to_string();
        // An arrow at an element position of `push` / `unshift` /
        // `fill` / `with` / `splice` is stored, not called — the
        // receiver's element type is its type, the same contextual
        // typing a literal element already gets.
        if apply_stored_elem_arm(ast, &name, &elem_ann, &closure_args, &mut updates) {
            continue;
        }
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
        // Per-method expected (param annotations, return annotation).
        // Spec §23.1.3 callback positions — (elem, index, srcArray);
        // the apply step writes only as many as the closure declares.
        // srcArray is `any[]` (kind-aware Arr<Any> view) — see
        // `hof_pos_anns` in infer_closure_params_helpers for why.
        let pos3 = |ret: &str| {
            Some((
                vec![elem_ann.clone(), "number".to_string(), "any[]".to_string()],
                ret.to_string(),
            ))
        };
        let expected: Option<(Vec<String>, String)> = match name.as_str() {
            "sort" => Some((vec![elem_ann.clone(), elem_ann.clone()], "number".into())),
            "map" => unreachable!("map handled by param-only arm above"),
            "filter" => pos3("boolean"),
            "forEach" => pos3("void"),
            "find" | "findLast" => pos3("boolean"),
            "findIndex" | "findLastIndex" => pos3("boolean"),
            "some" | "every" => pos3("boolean"),
            "flatMap" => unreachable!("flatMap handled by param-only arm above"),
            "reduce" | "reduceRight" => {
                unreachable!("reduce/reduceRight handled by param-only arm above")
            }
            _ => None,
        };
        let Some(expected) = expected else { continue };
        for (_arg_idx, fn_name) in &closure_args {
            updates.insert(fn_name.clone(), expected.clone());
            if name != "sort" {
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
