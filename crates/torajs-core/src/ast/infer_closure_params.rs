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
//!   `__fn(P|..)->R`-annotated param at a closure arg position
//!   projects its spellings onto the lifted closure's params/ret.

use super::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use super::infer_closure_typevars::{mentions_any_word, resolve_call_site_typevars};
use super::{Ast, Expr, ExprId, Stmt};
use crate::num_width::{fn_type_canon, split_fn_type};

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
/// - literal shapes — [`infer_lit_ann`].
fn resolve_receiver_ann(
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

    let all_anns = build_ann_table(ast);

    // Map from lifted closure fn_name → (param annotations, return
    // annotation). Filled by walking call sites; applied at the end
    // (deferred so we don't mutate ast.stmts mid-walk).
    let mut updates: HashMap<String, (Vec<String>, String)> = HashMap::new();

    // fn name → positional param annotations, for the user-fn callee
    // hint arm (RFC 20260705 chunk 554 — `apply((n) => n + 1, 41)`).
    // fn name → type params, so a generic callee's hint spellings get
    // a call-site typevar resolution pass before projection.
    let mut fn_param_pos_anns: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            type_params,
            ..
        } = s
        {
            fn_param_pos_anns.insert(
                name.clone(),
                params.iter().map(|p| p.type_ann.clone()).collect(),
            );
            if !type_params.is_empty() {
                fn_type_params.insert(name.clone(), type_params.clone());
            }
        }
    }

    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else {
            continue;
        };
        let callee = *callee;
        let args = args.clone();
        let mut closure_args: Vec<(usize, String)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            // Two shapes after lift_arrow_fns:
            //   - `Expr::Closure { fn_name, captures }` for arrows that
            //     captured outer-scope bindings.
            //   - `Expr::Ident(fn_name)` for arrows with no captures
            //     (lift emits a bare ident pointing at the lifted
            //     FnDecl). Both cases must be probed for inference.
            match ast.get_expr(*a) {
                Expr::Closure { fn_name, .. } => {
                    closure_args.push((i, fn_name.clone()));
                }
                Expr::Ident(n) if n.starts_with("__closure_") => {
                    closure_args.push((i, n.clone()));
                }
                _ => {}
            }
        }
        if closure_args.is_empty() {
            continue;
        }
        // User-fn callee: an `__fn(P|..)->R`-annotated param at a
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
            if let Some(pos_anns) = fn_param_pos_anns.get(fname) {
                let subst = fn_type_params
                    .get(fname)
                    .map(|tps| resolve_call_site_typevars(ast, tps, pos_anns, &args));
                let tps = fn_type_params.get(fname);
                for (arg_idx, fn_name) in &closure_args {
                    let Some(Some(pann)) = pos_anns.get(*arg_idx) else {
                        continue;
                    };
                    let Some(canon) = fn_type_canon(pann) else {
                        continue;
                    };
                    let Some((param_spellings, ret_spelling)) = split_fn_type(&canon) else {
                        continue;
                    };
                    let mut param_anns: Vec<String> =
                        param_spellings.iter().map(|s| s.to_string()).collect();
                    let mut ret_ann = ret_spelling.to_string();
                    if let (Some(subst), Some(tps)) = (&subst, tps) {
                        for a in param_anns.iter_mut() {
                            *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, subst);
                        }
                        ret_ann =
                            crate::ssa_lower_generics_monomorph::substitute_in_ann(&ret_ann, subst);
                        if param_anns.iter().any(|a| mentions_any_word(a, tps))
                            || mentions_any_word(&ret_ann, tps)
                        {
                            continue;
                        }
                    }
                    updates.insert(fn_name.clone(), (param_anns, ret_ann));
                }
            }
            continue;
        }
        // Member(obj, method) — the Array-method path.
        let Expr::Member { obj, name } = ast.get_expr(callee).clone() else {
            continue;
        };
        let obj_ann = resolve_receiver_ann(ast, obj, &all_anns);
        let Some(ann) = obj_ann else { continue };
        // `<string>.replace(re, cb)` / `replaceAll(re, cb)` — the
        // replacer receives `(match, ...captures, offset, input)`
        // (§22.1.3.19 GetSubstitution). The match (user param 0) is
        // always a String, so infer it — that lets the regex-lane
        // cb-sig check (`[Str; N+1] -> Str`) accept an otherwise-untyped
        // (Any) arrow's common `(m) => …` shape. Only the first user
        // param is annotated (a single-element ann list), so a
        // multi-param callback's trailing offset (number) / input
        // (string) stay untyped and that shape keeps hitting the lane's
        // reject (never mis-typed) — arity-aware inference is a follow-up.
        if ann == "string" && matches!(name.as_str(), "replace" | "replaceAll") {
            for (_arg_idx, fn_name) in &closure_args {
                updates.insert(fn_name.clone(), (vec!["string".into()], "string".into()));
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
        // Only handle T[] receivers for the known Array methods.
        let Some(elem_ann) = ann.strip_suffix("[]") else {
            continue;
        };
        let elem_ann = elem_ann.to_string();
        // Per-method expected (param annotations, return annotation).
        let expected: Option<(Vec<String>, String)> = match name.as_str() {
            "sort" => Some((vec![elem_ann.clone(), elem_ann.clone()], "number".into())),
            "map" => Some((vec![elem_ann.clone()], elem_ann.clone())),
            "filter" => Some((vec![elem_ann.clone()], "boolean".into())),
            "forEach" => Some((vec![elem_ann.clone()], "void".into())),
            "find" | "findLast" => Some((vec![elem_ann.clone()], "boolean".into())),
            "findIndex" | "findLastIndex" => Some((vec![elem_ann.clone()], "boolean".into())),
            "some" | "every" => Some((vec![elem_ann.clone()], "boolean".into())),
            "flatMap" => {
                // Return is `T[]` (flattened); inner cb returns array.
                Some((vec![elem_ann.clone()], format!("{elem_ann}[]")))
            }
            "reduce" | "reduceRight" => {
                // (acc, cur) => acc — caller supplies the seed; without
                // type-tracking the seed type, assume elem-typed accum
                // (works for sum/max/etc.).
                Some((vec![elem_ann.clone(), elem_ann.clone()], elem_ann.clone()))
            }
            _ => None,
        };
        let Some(expected) = expected else { continue };
        for (_arg_idx, fn_name) in &closure_args {
            updates.insert(fn_name.clone(), expected.clone());
        }
    }

    apply_closure_ann_updates(ast, &updates);
}

/// Callback param/return annotations for `forEach` on a `Map<K|V>` /
/// `Set<T>` receiver ann (the flat generic spelling). None for any
/// other method or receiver shape — Map/Set carry no other
/// callback-bearing methods.
fn mapset_foreach_expected(ann: &str, method: &str) -> Option<(Vec<String>, String)> {
    if method != "forEach" {
        return None;
    }
    if let Some(inner) = ann.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [k, v] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![v.to_string(), k.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    if let Some(inner) = ann.strip_prefix("Set<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner, true);
        let [t] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![t.to_string(), t.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    None
}

/// Per-name → type-annotation table feeding receiver resolution.
/// Walk all top-level FnDecl bodies gathering param + let-decl
/// annotations (the same name may appear in multiple fns; call-site
/// inference resolves the right binding via the enclosing fn), plus:
///
/// - V3-18 m1.h.23 — top-level let decls (the synthetic `main`
///   wraps these at ssa_lower time, but at this AST pass they sit at
///   ast.stmts level, so the FnDecl-only walk misses them; without
///   this `let arr = [1,2,3]; arr.find(x => ...)` can't infer x).
/// - Inferred-from-init shape: `let arr = [<lit>, ...]` infers
///   arr's annotation as `<lit_ty>[]` so .map / .filter on
///   unannotated lets still get param inference.
fn build_ann_table(ast: &Ast) -> std::collections::HashMap<String, String> {
    use std::collections::HashMap;
    let mut all_anns: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                }
            }
            collect_let_anns(body, &mut all_anns);
        }
    }
    collect_let_anns(&ast.stmts, &mut all_anns);
    let mut inferred_inits: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, &mut inferred_inits);
    for (k, v) in inferred_inits {
        all_anns.entry(k).or_insert(v);
    }
    all_anns
}

/// Apply the deferred updates: mutate each lifted FnDecl's params +
/// return type (deferred so the call-site walk doesn't mutate
/// ast.stmts mid-walk).
fn apply_closure_ann_updates(
    ast: &mut Ast,
    updates: &std::collections::HashMap<String, (Vec<String>, String)>,
) {
    if updates.is_empty() {
        return;
    }
    for stmt in &mut ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = stmt
            && let Some((new_param_anns, new_ret_ann)) = updates.get(name)
        {
            // First param of a lifted closure is `__env`; user params
            // start at index 1.
            let user_start = if params.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            for (i, ann) in new_param_anns.iter().enumerate() {
                let pidx = user_start + i;
                if let Some(p) = params.get_mut(pidx)
                    && p.type_ann.is_none()
                {
                    p.type_ann = Some(ann.clone());
                }
            }
            if return_type.is_none() && body_returns_value(body) {
                *return_type = Some(new_ret_ann.clone());
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
fn body_returns_value(body: &[Stmt]) -> bool {
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
