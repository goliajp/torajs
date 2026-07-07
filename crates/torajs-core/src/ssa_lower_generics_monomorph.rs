//! Generics monomorphization pre-pass (M3). Extracted from `ssa_lower.rs`
//! chunk 364 — the substitute + monomorphize + rewrite-tvdefault family.
//!
//! Pipeline (called from `lower_with_arity` after typecheck):
//!   1. `monomorphize_generics(ast, generic_call_sites)` — produces
//!      specialized `Stmt::FnDecl`s for each unique `(name, type_args)`
//!      tuple, plus a `CallRetargets` map rewriting each generic call
//!      site's callee.
//!   2. `substitute_in_ann` — bare-word type-param substitution inside
//!      annotation strings. Also used by:
//!        * `num_width::alias` for width-aware alias resolution
//!        * `ssa_lower_parse_type` for FnAnn subst
//!   3. `rewrite_tvdefault_in_*` — replaces the `__tvdefault__<T>` marker
//!      Idents (planted by class-factory default-init) with the concrete
//!      default expression for the substituted type.
//!
//! Cross-file deps still in `ssa_lower.rs`:
//!   - `deep_clone_stmt` (fresh ExprIds per mono instance)
//!   - `rewrite_inner_generic_calls` (transitive rewrite)
//!   Both are `pub(crate)` so this sibling can call them via
//!   `crate::ssa_lower::{deep_clone_stmt, rewrite_inner_generic_calls}`.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{self as check_mod, GenericCallSites, type_to_ann};
use crate::ssa_lower::{CallRetargets, deep_clone_stmt, rewrite_inner_generic_calls};

/// Encode an annotation string into a name-safe form for use inside a
/// monomorphized fn name. `number` → `number`; `number[]` → `number_arr`;
/// `__fn(number)->number` → `fn_number_to_number`. Distinct user types
/// produce distinct strings so the cache key `(name, type_args)` resolves
/// to a unique mono fn.
pub(crate) fn name_safe(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => c,
            _ => '_',
        })
        .collect()
}

/// Replace bare-word occurrences of each `from` token with `to` inside an
/// annotation string. Word boundary = anything that isn't an alphanumeric
/// or `_`. Used by `monomorphize_generics` to rewrite a generic FnDecl's
/// type annotations into a concrete specialization (e.g. `T` → `number`,
/// `T[]` → `number[]`, `__fn(T)->T` → `__fn(number)->number`).
pub(crate) fn substitute_in_ann(ann: &str, subst: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ann.len());
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let is_word_start = c.is_ascii_alphabetic() || c == b'_';
        if !is_word_start {
            out.push(c as char);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let cc = bytes[i];
            if cc.is_ascii_alphanumeric() || cc == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        let word = &ann[start..i];
        if let Some((_, replacement)) = subst.iter().find(|(from, _)| from == word) {
            out.push_str(replacement);
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Substitute every type-param name in a `Stmt`'s body recursively.
/// Currently only `Stmt::LetDecl` and the immediate FnDecl signature
/// carry annotation strings; we walk into nested Block / If / While / For
/// bodies. `subst` is the (param → concrete-ann) list applied to every
/// `type_ann` Some(...) string encountered.
fn substitute_in_stmt(stmt: &mut Stmt, subst: &[(String, String)]) {
    match stmt {
        Stmt::LetDecl { type_ann, .. } => {
            if let Some(ann) = type_ann {
                *ann = substitute_in_ann(ann, subst);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            substitute_in_stmt(then_branch, subst);
            if let Some(eb) = else_branch {
                substitute_in_stmt(eb, subst);
            }
        }
        Stmt::While { body, .. } => substitute_in_stmt(body, subst),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                substitute_in_stmt(i, subst);
            }
            substitute_in_stmt(body, subst);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                substitute_in_stmt(s, subst);
            }
        }
        Stmt::FnDecl {
            params,
            return_type,
            body,
            ..
        } => {
            for p in params {
                if let Some(ann) = &mut p.type_ann {
                    *ann = substitute_in_ann(ann, subst);
                }
            }
            if let Some(rt) = return_type {
                *rt = substitute_in_ann(rt, subst);
            }
            for s in body {
                substitute_in_stmt(s, subst);
            }
        }
        // Expr / Return / Break / Continue / TypeDecl carry no annotation
        // strings worth substituting in the M3-minimal surface.
        _ => {}
    }
}

/// M3 — produce a monomorphized FnDecl for each unique
/// `(generic_name, type_args)` tuple in `generic_call_sites`. Returns:
///   - `mono_decls`: the new specialized FnDecls (to be appended to
///     ast.stmts so pass 1 / 2 lower them as concrete fns)
///   - `call_retargets`: per-call-site mapping `ExprId → mono_name` so
///     the lowerer can rewrite each generic call's callee
///   - `generic_fn_names`: original generic-fn names (for pass 1 to skip)
pub(crate) fn monomorphize_generics(
    ast: &mut Ast,
    generic_call_sites: &GenericCallSites,
) -> (Vec<Stmt>, CallRetargets, std::collections::HashSet<String>) {
    let mut mono_decls: Vec<Stmt> = Vec::new();
    let mut call_retargets: CallRetargets = HashMap::new();
    let mut generic_fn_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Cache: (name, [annotation_strings]) → mono_name. Re-uses an existing
    // monomorphization when two call sites infer the same type args.
    let mut cache: HashMap<(String, Vec<String>), String> = HashMap::new();

    // Index original generic FnDecls by name. Cloned out so we can
    // mutate ast freely below without aliasing.
    let generics: HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator: _,
            } if !type_params.is_empty() => Some((
                name.clone(),
                (
                    type_params.clone(),
                    params.clone(),
                    return_type.clone(),
                    body.clone(),
                ),
            )),
            _ => None,
        })
        .collect();
    for k in generics.keys() {
        generic_fn_names.insert(k.clone());
    }

    // Worklist: (callee_name, arg_anns) — pending monomorphizations to
    // emit. Seeded from generic_call_sites; grown by recursive walk
    // over each just-emitted body.
    let mut worklist: std::collections::VecDeque<(String, Vec<String>)> =
        std::collections::VecDeque::new();
    for (eid, (callee_name, type_args)) in generic_call_sites {
        // Width-aware ann selection: for each type-arg that resolved to
        // `Type::Number`, walk the arg positions whose param annotation
        // names this type-param and pick "f64" if any arg statically
        // lowers to f64 (Math.* call, decimal literal, etc.). Otherwise
        // keep the default "number" → I64. This lets one generic fn
        // serve both `check<T=Number>(1, 2)` (I64 mono) and
        // `check<T=Number>(Math.abs(-1), 1)` (F64 mono) cleanly.
        let widths: Vec<crate::num_width::NumWidth> =
            crate::num_width::compute_typevar_widths(ast, *eid, callee_name, type_args, &generics);
        // Closure-shape-aware ann selection (same mechanism as the
        // width pass): `check::Type::Function` carries no
        // closure-vs-bare-fn distinction, so `type_to_ann` always
        // answers `__fn(` — but a closure ARG instantiating that
        // type-param needs a `__cls(` slot (env-block ptr, env-first
        // CallIndirect). Without the flip the mono body's call arm
        // treats the env ptr as a bare fn ptr and jumps into it
        // (SIGBUS). The 153-pass `__fn(`→`__cls(` infection can't
        // see these anns — monomorphization runs after typecheck.
        let cls_shapes: Vec<bool> =
            crate::ssa_lower_generics_mono_shapes::compute_typevar_closure_shapes(
                ast,
                *eid,
                callee_name,
                type_args,
                &generics,
            );
        let arg_anns: Vec<String> = type_args
            .iter()
            .zip(widths.iter())
            .zip(cls_shapes.iter())
            .map(|((ty, w), is_cls)| {
                if matches!(ty, check_mod::Type::Number)
                    && matches!(w, crate::num_width::NumWidth::F64)
                {
                    "f64".into()
                } else {
                    let ann = type_to_ann(ty);
                    if *is_cls && ann.starts_with("__fn(") {
                        format!("__cls({}", &ann["__fn(".len()..])
                    } else {
                        ann
                    }
                }
            })
            .collect();
        let cache_key = (callee_name.clone(), arg_anns.clone());
        if !cache.contains_key(&cache_key) {
            // Reserve mono name early so cycles break.
            let suffix: Vec<String> = arg_anns.iter().map(|a| name_safe(a)).collect();
            let mono_name = format!("{}$$_{}", callee_name, suffix.join("_"));
            cache.insert(cache_key.clone(), mono_name.clone());
            worklist.push_back((callee_name.clone(), arg_anns.clone()));
        }
        let mono_name = cache[&cache_key].clone();
        call_retargets.insert(*eid, mono_name);
    }
    while let Some((callee_name, arg_anns)) = worklist.pop_front() {
        let cache_key = (callee_name.clone(), arg_anns.clone());
        let mono_name = cache[&cache_key].clone();
        let Some((type_params, params, return_type, body)) = generics.get(&callee_name) else {
            continue;
        };
        let subst: Vec<(String, String)> = type_params
            .iter()
            .cloned()
            .zip(arg_anns.iter().cloned())
            .collect();
        let mut new_params: Vec<Param> = params.clone();
        for p in new_params.iter_mut() {
            if let Some(ann) = &mut p.type_ann {
                *ann = substitute_in_ann(ann, &subst);
            }
        }
        let new_return_type = return_type.as_ref().map(|rt| substitute_in_ann(rt, &subst));
        // Deep-clone the body's expression graph so each mono body has
        // FRESH ExprIds. Without this, multiple instantiations of the
        // same generic share one expression arena and the
        // transitive-rewrite step below would overwrite each other.
        let mut new_body: Vec<Stmt> = body.iter().map(|s| deep_clone_stmt(ast, s)).collect();
        for s in new_body.iter_mut() {
            substitute_in_stmt(s, &subst);
        }
        // Rewrite `__tvdefault__T` marker Idents in object-literal field
        // initializers to the concrete default for the substituted type.
        // These markers are emitted by `default_init_for_type` for
        // generic-class fields whose type is a TypeVar; without this
        // rewrite the ObjectLit's field types wouldn't match the
        // factory's let-decl type ann after substitution.
        for s in new_body.iter() {
            rewrite_tvdefault_in_stmt(ast, s, &subst);
        }
        // Transitive rewrite: walk the freshly-substituted body for
        // Call expressions whose callee is a generic fn sharing the
        // SAME type_params name list. Reuse the outer subst (matching
        // by position), rewrite the callee Ident to the mono name,
        // and queue the inner instantiation. Class methods all share
        // the class's type_params, so this covers __cm_C__m, the
        // factory __new_C, and the ctor uniformly.
        rewrite_inner_generic_calls(
            ast,
            &mut new_body,
            &generics,
            type_params,
            &arg_anns,
            &mut cache,
            &mut worklist,
        );
        mono_decls.push(Stmt::FnDecl {
            name: mono_name,
            type_params: Vec::new(),
            params: new_params,
            return_type: new_return_type,
            body: new_body,
            is_generator: false,
        });
    }
    (mono_decls, call_retargets, generic_fn_names)
}

/// Walk a Stmt's expression graph and rewrite any `__tvdefault__<T>`
/// marker Ident into the proper concrete default expression for the
/// substituted type T. Operates IN PLACE on the AST arena (so the
/// caller's deep-cloned body sees the rewrite).
fn rewrite_tvdefault_in_stmt(ast: &mut Ast, s: &Stmt, subst: &[(String, String)]) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) => rewrite_tvdefault_in_expr(ast, *eid, subst),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                rewrite_tvdefault_in_expr(ast, *eid, subst);
            }
        }
        Stmt::LetDecl { init, .. } => rewrite_tvdefault_in_expr(ast, *init, subst),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, then_branch, subst);
            if let Some(eb) = else_branch {
                rewrite_tvdefault_in_stmt(ast, eb, subst);
            }
        }
        Stmt::While { cond, body } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::DoWhile { body, cond } => {
            rewrite_tvdefault_in_stmt(ast, body, subst);
            rewrite_tvdefault_in_expr(ast, *cond, subst);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rewrite_tvdefault_in_stmt(ast, i, subst);
            }
            if let Some(c) = cond {
                rewrite_tvdefault_in_expr(ast, *c, subst);
            }
            if let Some(s2) = step {
                rewrite_tvdefault_in_expr(ast, *s2, subst);
            }
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_tvdefault_in_expr(ast, *scrutinee, subst);
            for c in cases {
                rewrite_tvdefault_in_expr(ast, c.value, subst);
                for s in &c.body {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
            if let Some(db) = default {
                for s in db {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            for s in catch_body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
        }
        _ => {}
    }
}

fn rewrite_tvdefault_in_expr(ast: &mut Ast, eid: ExprId, subst: &[(String, String)]) {
    // First detect the marker; rewrite in place if found.
    if let Expr::Ident(name) = ast.get_expr(eid) {
        if let Some(tv) = name.strip_prefix("__tvdefault__") {
            // Find the substituted concrete type for this TypeVar.
            for (tp_name, ann) in subst {
                if tp_name == tv {
                    let new_expr = match ann.as_str() {
                        "number" | "i64" => Expr::Number(0.0),
                        "f64" => Expr::Number(0.5), // forces fract() != 0 → ConstF64
                        "boolean" => Expr::Bool(false),
                        "string" => Expr::String(String::new()),
                        _ => Expr::Number(0.0),
                    };
                    ast.exprs[eid.0 as usize] = new_expr;
                    return;
                }
            }
        }
    }
    // Recurse into sub-expressions.
    let kind = ast.get_expr(eid).clone();
    match kind {
        Expr::BinOp { left, right, .. } => {
            rewrite_tvdefault_in_expr(ast, left, subst);
            rewrite_tvdefault_in_expr(ast, right, subst);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            rewrite_tvdefault_in_expr(ast, expr, subst);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
        }
        Expr::Call { callee, args } => {
            rewrite_tvdefault_in_expr(ast, callee, subst);
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
        }
        Expr::Assign { target, value } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
            rewrite_tvdefault_in_expr(ast, value, subst);
        }
        Expr::Index { obj, index } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
            rewrite_tvdefault_in_expr(ast, index, subst);
        }
        Expr::Array(els) => {
            for e in els {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_tvdefault_in_expr(ast, cond, subst);
            rewrite_tvdefault_in_expr(ast, then_branch, subst);
            rewrite_tvdefault_in_expr(ast, else_branch, subst);
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_tvdefault_in_expr(ast, lhs, subst);
            rewrite_tvdefault_in_expr(ast, rhs, subst);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
        }
        Expr::PostIncr { target, .. } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
        }
        _ => {}
    }
}
