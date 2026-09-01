//! Support helpers for [`super::infer_closure_params`] — extracted
//! as the rotation-196 file-size sweep. The parent had drifted to
//! 508 prod LOC over the recent .map/.flatMap wedge additions;
//! `mapset_foreach_expected` (Map/Set forEach param shape) and
//! `build_ann_table` (per-name annotation gather over top-level
//! FnDecls + let-inits) are self-contained and share zero mutable
//! state with the main walker, so cluster naturally into a sibling.
//! Verbatim moves; signatures / semantics unchanged.
//!
//! Rotation 197 sweep — `collect_fn_decl_metadata` (four side-channel
//! HashMap gather), `apply_user_fn_callee_hint` (chunk-554 face ②
//! user-fn callee arm), and `apply_hof_param_only_arm` (map / flatMap /
//! reduce / reduceRight param-only seed) also moved here to bring the
//! parent's `infer_anonymous_closure_params` fn under the 200-line hard
//! limit without breaking the 500-line file cap.

use std::collections::HashMap;

use super::infer_closure_params_apply::body_returns_value;
use super::infer_closure_typevars::{mentions_any_word, resolve_call_site_typevars};
use super::infer_return_ann;
use crate::ast::infer_closure_lets::{collect_let_anns, collect_let_init_anns};
use crate::ast::{Ast, ExprId, Stmt};
use crate::num_width::{fn_type_canon, split_fn_type};

/// Callback param/return annotations for `forEach` on a `Map<K|V>` /
/// `Set<T>` receiver ann (the flat generic spelling). None for any
/// other method or receiver shape — Map/Set carry no other
/// callback-bearing methods.
pub(super) fn mapset_foreach_expected(ann: &str, method: &str) -> Option<(Vec<String>, String)> {
    if method != "forEach" {
        return None;
    }
    if let Some(inner) = ann.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner);
        let [k, v] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![v.to_string(), k.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    if let Some(inner) = ann.strip_prefix("Set<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner);
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
/// Read `ann` as a fn-type canonical spelling, chasing bare type
/// aliases to get there: `type F = (n: number) => number` parks its RHS
/// under an `__alias__` sentinel field, so the annotation on `const g:
/// F` is the bare name `F` and canonicalizes to nothing on its own.
/// Bounded so a `type A = B; type B = A` pair cannot spin.
pub(super) fn resolve_fn_type_ann(ast: &Ast, ann: &str) -> Option<String> {
    let mut cur = ann.to_string();
    for _ in 0..8 {
        if let Some(canon) = fn_type_canon(&cur) {
            return Some(canon);
        }
        cur = ast.stmts.iter().find_map(|s| match s {
            Stmt::TypeDecl { name, fields, .. } if *name == cur => fields
                .iter()
                .find(|(f, _)| f == "__alias__")
                .map(|(_, a)| a.clone()),
            _ => None,
        })?;
    }
    None
}

/// Read `ann` as a fn type and answer the (param spellings, return
/// spelling) a lifted closure's own annotations take from it.
///
/// The single place that asks "what does this annotation say a
/// function's parameters are", for every position that types an arrow
/// by the type of the slot it lands in: an annotated binding, a field
/// or element of an annotated container, a declared return type, and
/// an element stored through an array mutator. `None` for an
/// annotation that is not a fn type — the arrow then keeps whatever
/// its own params say.
pub(super) fn project_fn_type_ann(ast: &Ast, ann: &str) -> Option<(Vec<String>, String)> {
    let canon = resolve_fn_type_ann(ast, ann)?;
    let (param_spellings, ret_spelling) = split_fn_type(&canon)?;
    Some((
        param_spellings.iter().map(|s| s.to_string()).collect(),
        ret_spelling.to_string(),
    ))
}

/// Project each `lifted closure → binding annotation` hint onto the
/// closure's params and return, the let-init counterpart of
/// [`apply_user_fn_callee_hint`]. Both land in the same `updates` map
/// and go through the same applier, which is what makes an explicitly
/// annotated param win, a shorter param list not overrun, and a
/// void-bodied closure keep its Void return.
pub(super) fn seed_let_ann_hints(
    ast: &Ast,
    closure_let_hints: &HashMap<String, String>,
    updates: &mut HashMap<String, (Vec<String>, String)>,
) {
    for (fn_name, ann) in closure_let_hints {
        if let Some(projected) = project_fn_type_ann(ast, ann) {
            updates.insert(fn_name.clone(), projected);
        }
    }
}

/// A container literal written at an argument position takes the type
/// that position declares, the same way one written as an initializer
/// takes its binding's.
///
/// `take([(n) => n + 1])` against `take(fs: ((n: number) => number)[])`
/// was loud about it ("expected Array(Function([Number], Number)), got
/// Array(Function([Any], Any))"); the object form `take({ f: (n) => n
/// + 1 })` was not loud at all and answered its own argument back.
///
/// The walk into the literal is the one an annotated binding already
/// uses — a field takes its declared field type, an element takes the
/// array's element type, and a nested literal recurses.
pub(super) fn seed_container_arg_hints(
    ast: &Ast,
    param_ann: &str,
    arg: ExprId,
    updates: &mut HashMap<String, (Vec<String>, String)>,
) {
    let mut hints: HashMap<String, String> = HashMap::new();
    crate::ast::infer_closure_lets::seed_container_field_hints(ast, param_ann, arg, &mut hints);
    for (fn_name, ann) in hints {
        if let Some(projected) = project_fn_type_ann(ast, &ann) {
            updates.insert(fn_name, projected);
        }
    }
}

/// The argument positions at which an Array method holds an *element*
/// — a value stored into the receiver array — rather than a callback
/// it invokes with elements. `None` for every other method.
///
/// One list, asked two ways (is this method one of them, and does this
/// position hold an element), so the two answers cannot drift apart.
fn stored_elem_args(method: &str) -> Option<ElemArgs> {
    match method {
        // `push(...items)` / `unshift(...items)` — §23.1.3.23 / .34.
        "push" | "unshift" => Some(ElemArgs::All),
        // `fill(value, start?, end?)` — §23.1.3.7.
        "fill" => Some(ElemArgs::At(0)),
        // `with(index, value)` — §23.1.3.39.
        "with" => Some(ElemArgs::At(1)),
        // `splice(start, deleteCount, ...items)` — §23.1.3.36.
        "splice" => Some(ElemArgs::From(2)),
        _ => None,
    }
}

/// Which of a method's argument positions hold elements.
enum ElemArgs {
    All,
    At(usize),
    From(usize),
}

impl ElemArgs {
    fn holds(&self, arg_idx: usize) -> bool {
        match *self {
            ElemArgs::All => true,
            ElemArgs::At(i) => arg_idx == i,
            ElemArgs::From(i) => arg_idx >= i,
        }
    }
}

/// Array-mutator element arm — an arrow handed to `push` / `unshift` /
/// `fill` / `with` / `splice` at an element position is not a callback
/// being described, it is a value being stored, so the receiver's
/// element type IS that arrow's type.
///
/// It is the same contextual typing an arrow written as a literal
/// element already gets, reached one call later, and it was the only
/// way in with no context at all: an un-annotated param then takes its
/// default, the call site still dispatches through the declared
/// signature, and the two disagree about what a parameter is. Loudly
/// for the mutators that type-check their items (`fill` / `splice` /
/// `concat` answered "got Function([Any], Any)"), silently for the
/// ones that do not — `fs.push((n) => n + 1); fs[0](3)` answered
/// -562949953421311.
///
/// Returns true for those five methods — none of them is
/// callback-bearing, so the caller skips the per-method callback table
/// either way.
pub(super) fn apply_stored_elem_arm(
    ast: &Ast,
    name: &str,
    elem_ann: &str,
    closure_args: &[(usize, String)],
    updates: &mut HashMap<String, (Vec<String>, String)>,
) -> bool {
    let Some(elem_args) = stored_elem_args(name) else {
        return false;
    };
    if let Some(projected) = project_fn_type_ann(ast, elem_ann) {
        for (arg_idx, fn_name) in closure_args {
            if elem_args.holds(*arg_idx) {
                updates.insert(fn_name.clone(), projected.clone());
            }
        }
    }
    true
}

pub(super) fn build_ann_table(ast: &Ast) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut all_anns: HashMap<String, String> = HashMap::new();
    // Lifted-closure name → the fn-type annotation of the binding it
    // initializes, for contextual param typing (see `collect_let_anns`).
    let mut closure_hints: HashMap<String, String> = HashMap::new();
    // Name → declared return type, for a binding whose initializer is
    // a call. Only declared returns: a sniffed one is a guess, and
    // this table is read as if it were an annotation.
    let mut declared_rets: HashMap<String, String> = HashMap::new();
    // Bodies that have a `__this` of their own, with the class it is
    // annotated as — replayed after the program-wide assignment pass.
    let mut this_scopes: Vec<(&[Stmt], String)> = Vec::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            body,
            return_type,
            ..
        } = s
        {
            if let Some(rt) = return_type {
                declared_rets.insert(name.clone(), rt.clone());
            }
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                    if p.name == "__this" {
                        this_scopes.push((body, ann.clone()));
                    }
                }
            }
            collect_let_anns(ast, body, &mut all_anns, &mut closure_hints);
            if let Some(ret_ann) = return_type {
                super::infer_closure_lets::seed_return_hints(
                    ast,
                    body,
                    ret_ann,
                    &mut closure_hints,
                );
            }
        }
    }
    collect_let_anns(ast, &ast.stmts, &mut all_anns, &mut closure_hints);
    let mut inferred_inits: HashMap<String, String> = HashMap::new();
    collect_let_init_anns(ast, &ast.stmts, &declared_rets, &mut inferred_inits);
    for (k, v) in inferred_inits {
        all_anns.entry(k).or_insert(v);
    }
    // Assignment targets last: resolving one reads the finished
    // annotation table (`fs[0] = cb` needs to know what `fs` is).
    crate::ast::infer_closure_lets::seed_assign_hints(ast, &all_anns, &mut closure_hints);
    // ...except through `__this`, which names a different object in
    // every function and so cannot be answered from a table keyed by
    // bare name. See `infer_closure_this_scope::seed_this_assign_hints`.
    for (body, this_ann) in this_scopes {
        crate::ast::infer_closure_this_scope::seed_this_assign_hints(
            ast,
            body,
            &this_ann,
            &mut closure_hints,
        );
    }
    (all_anns, closure_hints)
}

/// Walk `ast.stmts` for FnDecls and pre-compute the four per-FnDecl side
/// channels the main call-site loop reads:
///   - `fn_param_pos_anns`: name → positional param annotations (chunk-554
///     user-fn callee hint arm).
///   - `fn_type_params`: name → generic type params (chunk-682 hint typevar
///     resolution).
///   - `fn_user_param_count`: name → user-param count excluding `__env`
///     (`<string>.replace(re, cb)` shape).
///   - `fn_ret_anns`: name → resolved (or sniffed) return ann (Promise
///     chain propagation for `.then(prev_cb).then(next_cb)`).
pub(super) fn collect_fn_decl_metadata(
    ast: &Ast,
) -> (
    HashMap<String, Vec<Option<String>>>,
    HashMap<String, Vec<String>>,
    HashMap<String, usize>,
    HashMap<String, Option<String>>,
) {
    let mut fn_param_pos_anns: HashMap<String, Vec<Option<String>>> = HashMap::new();
    let mut fn_type_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut fn_user_param_count: HashMap<String, usize> = HashMap::new();
    let mut fn_ret_anns: HashMap<String, Option<String>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl {
            name,
            params,
            type_params,
            return_type,
            body,
            ..
        } = s
        {
            fn_param_pos_anns.insert(
                name.clone(),
                params.iter().map(|p| p.type_ann.clone()).collect(),
            );
            fn_user_param_count.insert(
                name.clone(),
                params.iter().filter(|p| p.name != "__env").count(),
            );
            if !type_params.is_empty() {
                fn_type_params.insert(name.clone(), type_params.clone());
            }
            let ret = return_type.clone().or_else(|| {
                if body_returns_value(body) {
                    infer_return_ann(&ast.exprs, body, params, &HashMap::new())
                } else {
                    None
                }
            });
            fn_ret_anns.insert(name.clone(), ret);
        }
    }
    // r549 — `const t = (f: () => unknown) => f()` is called as
    // `t(() => 1)`: the user-fn callee hint keys on the callee ident,
    // so the lifted decl's `__fn(` spellings never reached the
    // callback (it stayed `-> i64` while the param's sig said
    // `-> any`; the env-first call read the i64 as an AnyValue —
    // EXIT 139). Mirror each closure-let alias's metadata under the
    // ident, with the lifted `__env` / `__this` prefix shifted off so
    // the positions line up with the call's args.
    for (ident, (fname, shift)) in crate::ast_closure_param_tag_collect::closure_let_aliases(ast) {
        if let Some(pos) = fn_param_pos_anns.get(&fname) {
            let shifted: Vec<Option<String>> = pos.iter().skip(shift).cloned().collect();
            fn_param_pos_anns.entry(ident.clone()).or_insert(shifted);
        }
        if let Some(tp) = fn_type_params.get(&fname) {
            let tp = tp.clone();
            fn_type_params.entry(ident).or_insert(tp);
        }
    }
    (
        fn_param_pos_anns,
        fn_type_params,
        fn_user_param_count,
        fn_ret_anns,
    )
}

/// User-fn callee arm — an `__fn(P|..)->R`-annotated param at a closure
/// arg position hints the lifted closure's own param/ret annotations
/// (chunk 554 face ②).
///
/// Generic callee (chunk 682) — the hint spellings may mention the
/// callee's type params (`g<T>(cb: (...args: T[]) => T, x: T)`);
/// projecting a raw `T` onto the lifted closure trips `build_fn_type`
/// with "unknown return type `T`" (the typevar is out of scope there).
/// Resolve typevars from the same call site first (a bare-typevar param
/// position holding a literal arg pins it: `x: T` + `21` → T=number);
/// a spelling still mentioning an unresolved typevar after substitution
/// is not projected.
pub(super) fn apply_user_fn_callee_hint(
    ast: &Ast,
    fname: &str,
    args: &[ExprId],
    closure_args: &[(usize, String)],
    fn_param_pos_anns: &HashMap<String, Vec<Option<String>>>,
    fn_type_params: &HashMap<String, Vec<String>>,
    updates: &mut HashMap<String, (Vec<String>, String)>,
) {
    let Some(pos_anns) = fn_param_pos_anns.get(fname) else {
        return;
    };
    let subst = fn_type_params
        .get(fname)
        .map(|tps| resolve_call_site_typevars(ast, tps, pos_anns, args));
    let tps = fn_type_params.get(fname);
    // A container literal at an argument position carries that
    // position's declared type inward; the arrows inside it are not
    // themselves the argument.
    for (arg_idx, arg) in args.iter().enumerate() {
        if let Some(Some(pann)) = pos_anns.get(arg_idx) {
            seed_container_arg_hints(ast, pann, *arg, updates);
        }
    }
    for (arg_idx, fn_name) in closure_args {
        let Some(Some(pann)) = pos_anns.get(*arg_idx) else {
            continue;
        };
        let Some(canon) = fn_type_canon(pann) else {
            continue;
        };
        let Some((param_spellings, ret_spelling)) = split_fn_type(&canon) else {
            continue;
        };
        let mut param_anns: Vec<String> = param_spellings.iter().map(|s| s.to_string()).collect();
        let mut ret_ann = ret_spelling.to_string();
        if let (Some(subst), Some(tps)) = (&subst, tps) {
            for a in param_anns.iter_mut() {
                *a = crate::ssa_lower_generics_monomorph::substitute_in_ann(a, subst);
            }
            ret_ann = crate::ssa_lower_generics_monomorph::substitute_in_ann(&ret_ann, subst);
            if param_anns.iter().any(|a| mentions_any_word(a, tps))
                || mentions_any_word(&ret_ann, tps)
            {
                continue;
            }
        }
        updates.insert(fn_name.clone(), (param_anns, ret_ann));
    }
}

// `apply_hof_param_only_arm` (map / flatMap / reduce / reduceRight
// param-only seeds) lives in the sibling
// `infer_closure_params_hof.rs` — moved when the cluster-#1 blade-2
// position seeds pushed this file past the 500-line cap.
