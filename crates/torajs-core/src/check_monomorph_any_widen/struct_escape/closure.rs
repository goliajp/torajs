//! The all-`any` twin of a lifted closure whose value may be called
//! through `any`.
//!
//! `const f: any = (s: string) => s.length; f(null)` enters the
//! closure's boxed adapter, and a string slot cannot hold `null`: the
//! body read `.length` off no string at all. §13.3.2.1 makes `s.length`
//! a [[Get]] on whatever `s` holds, so `f(null)` throws a TypeError and
//! `f(42)` answers `undefined`. A lifted body already wears the
//! `__env` face the adapter calls with, so the twin is one clone,
//! `<closure>$$anyws`, with every non-rest parameter `any` and its
//! return `any`, checked at the original's construction site with the
//! capture types recorded there and reading the original's env layout
//! (551-03, `widen_clone_origin`). The mono adapter tests each slot and
//! hands a non-fitting call to the twin's adapter
//! (`ssa_lower_boxed_entry::twin_adapter_of`).
//!
//! Only a closure whose value may leave typed calls gets one. These
//! construction sites are only ever called through a typed face the
//! checker holds to the closure's own parameter types:
//! - the callee of its own call;
//! - the init of a binding whose declared parameter types are the
//!   closure's and whose every reference is a call's callee;
//! - the callback of a typed array's `map` / `filter` / `reduce` /
//!   `reduceRight` / `forEach` / `find*` / `some` / `every`, which the
//!   lowering calls directly (`ssa_lower_call_arr_ho`,
//!   `ssa_lower_call_arr_predicate`; an `arguments`-reading callback
//!   has an argv face and no twin anyway);
//! - the comparator of a typed array's `sort` taking the element type:
//!   both sort lanes hand it only elements of that kind (`sort_holes`);
//! - the callback of `s.replace(re, cb)` on a string whose signature
//!   the kernel lane serves through the typed callback ABI
//!   (`ssa_lower_call_str_regex_emit::callback_stays_typed`, the same
//!   predicate that lane decides by); the view lane calls a view twin.
//!
//! A closure of a generic body is cloned per specialization
//! (`__closure_N$$…`); the clones are the closures that run, so they
//! are twinned and the retired original is not. The twins' inner
//! closures are cloned under [`inner_suffix`], never under the twin
//! suffix, which would read as the inner closure's own twin. The
//! suffix names the twin's original: the twin of a closure and the
//! twin of the closure it builds both clone the innermost one, and
//! one shared spelling would declare that clone twice.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{is_twin, twin_name, widenable_target};
use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::{Checker, Type};
use crate::check_local_info::LocalInfo;
use crate::check_monomorph::WorkItem;
use crate::check_monomorph_any_widen::{RetFace, WidenTarget, emit_widened_spec};
use crate::ssa_lower_generics_monomorph::Generics;

/// The suffix of the inner closures the twin of `owner` clones.
pub(in crate::check_monomorph_any_widen) fn inner_suffix(owner: &str) -> String {
    format!("$$anywsc_{owner}")
}

/// The all-`any` widen plan of a twin: every non-rest user slot.
pub(in crate::check_monomorph_any_widen) fn all_any(params: &[Param]) -> Vec<(usize, WidenTarget)> {
    params
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.is_rest)
        .map(|(i, _)| (i, WidenTarget::Scalar))
        .collect()
}

/// Mint the twin of every lifted closure that may be called through
/// `any` and declares a struct, string, array, `number` or `boolean`
/// parameter, and has none yet. Part of the widen fixpoint: a specialization drained
/// there constructs closures of its own. `retired` are the generic
/// originals, whose bodies never lower. True when one was minted.
#[allow(clippy::too_many_arguments)]
pub(in crate::check_monomorph_any_widen) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    retired: &HashSet<String>,
) -> bool {
    let mut minted = false;
    for name in loose_closures(c, owned_ast) {
        let twin = twin_name(&name);
        if retired.contains(&name)
            || c.globals.contains_key(&twin)
            || !c.globals.contains_key(&name)
        {
            continue;
        }
        let Some(params) = widenable_target(c, owned_ast, &name).map(|t| t.params) else {
            continue;
        };
        // the twin's `any` values would be stored into the typed slot of
        // a binding outside it, which lowering refuses; such a closure
        // keeps the coercion kernel
        if owned_ast.stmts.iter().any(|s| {
            matches!(s, Stmt::FnDecl { name: n, .. } if *n == name)
                && crate::ast::free_vars_write::writes_outer_binding(owned_ast, s)
        }) {
            continue;
        }
        // the twin is checked at the original's construction site, whose
        // scope (an enclosing body's locals) is gone by now: bind the
        // capture types recorded there
        let caps = c.closure_captures.get(&name).cloned().unwrap_or_default();
        c.scopes
            .push(caps.into_iter().map(|(n, ty)| (n, captured(ty))).collect());
        emit_widened_spec(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            &name,
            &twin,
            &inner_suffix(&name),
            &all_any(&params),
            RetFace::Any,
        );
        c.scopes.pop();
        minted = true;
    }
    minted
}

fn captured(ty: Type) -> LocalInfo {
    LocalInfo {
        ty,
        mutable: true,
        moved: false,
        borrowed: false,
        declared_class: None,
        builtin_mv: false,
        exec_result: false,
        fn_slot: None,
    }
}

/// The lifted closures, in arena order, with a construction site whose
/// value may be called other than through its typed face.
fn loose_closures(c: &Checker, ast: &Ast) -> Vec<String> {
    loose_values(c, ast, "__closure_")
}

/// [`loose_closures`] over the lifted bodies named `prefix…`.
pub(in crate::check_monomorph_any_widen) fn loose_values(
    c: &Checker,
    ast: &Ast,
    prefix: &str,
) -> Vec<String> {
    let callees: HashSet<ExprId> = ast
        .exprs
        .iter()
        .filter_map(|e| match e {
            Expr::Call { callee, .. } => Some(*callee),
            _ => None,
        })
        .collect();
    // names read anywhere other than as a callee; a name shared by
    // several bindings only makes more of them loose
    let read_as_value: HashSet<&str> = ast
        .exprs
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match e {
            Expr::Ident(n) if !callees.contains(&ExprId(i as u32)) => Some(n.as_str()),
            _ => None,
        })
        .collect();
    let mut bound: HashMap<ExprId, (String, Option<String>)> = HashMap::new();
    crate::ast::walk_stmts(&ast.stmts, &mut |s| {
        if let Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } = s
        {
            bound.insert(*init, (name.clone(), type_ann.clone()));
        }
    });
    let typed_callbacks = typed_callback_sites(c, ast);
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for (i, e) in ast.exprs.iter().enumerate() {
        let Expr::Closure { fn_name, .. } = e else {
            continue;
        };
        if !fn_name.starts_with(prefix) || is_twin(fn_name) {
            continue;
        }
        let id = ExprId(i as u32);
        let typed_only = callees.contains(&id)
            || typed_callbacks.contains(&id)
            || bound.get(&id).is_some_and(|(name, ann)| {
                !read_as_value.contains(name.as_str())
                    && ann.as_deref().is_none_or(|a| same_face(c, a, fn_name))
            });
        if !typed_only && seen.insert(fn_name.as_str()) {
            out.push(fn_name.clone());
        }
    }
    out
}

/// Does the binding annotation `ann` declare exactly the parameter
/// types of closure `fn_name`? A wider declared parameter
/// (`(s: any) => number` over `(s: string) => …`) lets a typed call
/// hand the closure what its slot cannot hold.
fn same_face(c: &Checker, ann: &str, fn_name: &str) -> bool {
    let declared =
        crate::check_type_ann::resolve_type_ann_full(ann, &c.aliases, &[], &c.generic_alias_decls);
    match (declared, c.globals.get(fn_name)) {
        (Some(Type::Function(d, _)), Some(Type::Function(p, _))) => d == *p,
        _ => false,
    }
}

/// The callback arguments the lowering calls through their typed face
/// only: a builtin array iteration on a typed array, and a string
/// `replace` whose callback the typed callback ABI serves. A receiver
/// or callee the checker typed `any` (an any receiver, a patched
/// prototype method) goes through the any lane instead.
fn typed_callback_sites(c: &Checker, ast: &Ast) -> HashSet<ExprId> {
    let mut out = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if c.expr_types.get(callee) == Some(&Type::Any) {
            continue;
        }
        let recv = c.expr_types.get(obj);
        let site = match name.as_str() {
            "map" | "filter" | "reduce" | "reduceRight" | "forEach" | "find" | "findIndex"
            | "findLast" | "findLastIndex" | "some" | "every" => {
                matches!(recv, Some(Type::Array(el)) if **el != Type::Any).then_some(0)
            }
            "sort" => match (recv, args.first().map(|a| ast.get_expr(*a))) {
                (Some(Type::Array(el)), Some(Expr::Closure { fn_name, .. }))
                    if **el != Type::Any =>
                {
                    matches!(c.globals.get(fn_name), Some(Type::Function(ps, _))
                        if ps.iter().all(|p| p == &**el))
                    .then_some(0)
                }
                _ => None,
            },
            "replace" | "replaceAll" if recv == Some(&Type::String) && args.len() >= 2 => {
                replace_stays_typed(c, ast, args).then_some(1)
            }
            _ => None,
        };
        if let Some(i) = site
            && let Some(&cb) = args.get(i)
            && matches!(ast.get_expr(cb), Expr::Closure { .. })
        {
            out.insert(cb);
        }
    }
    out
}

/// `s.replace(re, cb)` whose callback the kernel lane calls through the
/// typed callback ABI, by the lowering's own predicate. Only a string
/// slot is mapped onto its lowered type; any other parameter makes
/// the site loose.
fn replace_stays_typed(c: &Checker, ast: &Ast, args: &[ExprId]) -> bool {
    use crate::ssa::Type as Ssa;
    use crate::ssa_lower_call_str_regex_emit::{callback_stays_typed, static_replace_caps};
    let Expr::Closure { fn_name, .. } = ast.get_expr(args[1]) else {
        return false;
    };
    let Some(Type::Function(params, ret)) = c.globals.get(fn_name) else {
        return false;
    };
    let lowered = |t: &Type| {
        if *t == Type::String {
            Ssa::Str
        } else {
            Ssa::Any
        }
    };
    let params: Vec<Ssa> = params.iter().map(lowered).collect();
    let caps = static_replace_caps(ast, args[0]);
    // a pattern that is no RegExp takes the string-pattern lane
    if caps.is_none() && c.expr_types.get(&args[0]) != Some(&Type::RegExp) {
        return false;
    }
    callback_stays_typed(&params, lowered(ret), caps)
}
