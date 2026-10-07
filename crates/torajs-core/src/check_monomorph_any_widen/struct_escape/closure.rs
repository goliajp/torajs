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

use super::typed_sites::typed_callback_sites;
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
    let names = loose_closures(c, owned_ast);
    mint(
        c,
        owned_ast,
        generics,
        cache,
        worklist,
        call_retargets,
        mono_decls,
        retired,
        names,
        false,
    )
}

/// Mint the twin of each of the lifted closures `names` that has none
/// yet and can take one; `recv` admits a receiver head
/// ([`widenable_target`]). True when one was minted.
#[allow(clippy::too_many_arguments)]
pub(super) fn mint(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
    retired: &HashSet<String>,
    names: Vec<String>,
    recv: bool,
) -> bool {
    let mut minted = false;
    for name in names {
        let twin = twin_name(&name);
        if retired.contains(&name)
            || c.globals.contains_key(&twin)
            || !c.globals.contains_key(&name)
            || writes_typed_outer_binding(c, owned_ast, &name)
        {
            continue;
        }
        let Some(params) = widenable_target(c, owned_ast, &name, recv).map(|t| t.params) else {
            continue;
        };
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

/// Does the body of `name` write a binding outside it that cannot take
/// the twin's values? The twin's `any` values would be stored into a
/// container-typed slot (`Map<number, string> | undefined`), which
/// lowering refuses; such a closure keeps the coercion kernel. An `any`
/// binding takes them as they are, and a number / string / boolean one
/// takes what its own assignments compute: one that may store an `any`
/// element as it is was retyped `any` (`num_width::hidden_any`).
fn writes_typed_outer_binding(c: &Checker, ast: &Ast, name: &str) -> bool {
    let caps = c.closure_captures.get(name);
    let takes_twin_values = |w: &String| {
        matches!(
            caps.and_then(|cs| cs.iter().find(|(n, _)| n == w))
                .map(|(_, t)| t)
                .or_else(|| c.globals.get(w)),
            Some(Type::Any | Type::Number | Type::String | Type::Boolean)
        )
    };
    ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name: n, .. } if n == name)
            && !crate::ast::free_vars_write::outer_writes(ast, s)
                .iter()
                .all(takes_twin_values)
    })
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
