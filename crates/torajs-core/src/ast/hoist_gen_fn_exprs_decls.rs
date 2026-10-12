//! Nested generator DECLARATIONS that capture an enclosing local take
//! the generator-expression route (rotation 634).
//!
//! `desugar_generators_lift` lifts every nested `function*` to the
//! top level under a mangled name, and a top-level body is checked in
//! top-level scope — so a body reading the parent's local died at
//! check ("unknown identifier"), the same hole the plain nested-fn
//! lift had until `nested_fns_capture` split the capturing ones off.
//! This is that split for generators: a nested generator declaration
//! whose body has a free variable that is not a global becomes
//! `let <name> = function* (…) { … }`, marked in `gen_fn_exprs`
//! exactly as the parser marks a written generator expression — and
//! [`super::hoist_gen_fn_exprs`], which runs right after this, gives it
//! the wrapper route: captured names become call-time snapshot params,
//! a reassigned capture is read by reference. Whatever this leaves
//! behind reaches the lift unchanged.
//!
//! The binding rises in its list as far as its captures allow (see
//! [`convert_list`]), so a call above the declaration finds it live.
//!
//! The parser's `yield* g()` fast path bakes `__Gen_g` / `__step_g`
//! annotations at parse time; a converted `g` mints no such class,
//! so those annotations widen to `any` here — the same remap the
//! lift does for its mangled names, aimed at the dynamic tier instead.

use super::free_vars_hoisted_names::hoist_var_names;
use super::scope_chain::{DeclKey, chains_by_fn_decl, list_names, not_rebound};
use super::stmt_nested_lists::{for_each_nested_list, for_each_nested_vec_mut};
use super::{Ast, Expr, GenFnExprInfo, GenFnExprKind, Stmt};
use std::collections::{HashMap, HashSet};

/// Convert every capturing nested generator declaration in place.
/// `global_names` is the top-level binding set the expression hoist
/// pre-binds — a name in it is not a capture, unless a frame around
/// the declaration rebinds it (`scope_chain`).
pub(super) fn route_capturing_nested_gen_decls(ast: &mut Ast, global_names: &[String]) {
    let mut top = std::mem::take(&mut ast.stmts);
    let chains = chains_by_fn_decl(ast, &[&top]);
    let mut counts: HashMap<String, usize> = HashMap::new();
    count_gen_decls(&top, &mut counts);
    for e in &ast.exprs {
        if let Expr::ArrowFn { body, .. } = e {
            count_gen_decls(body, &mut counts);
        }
    }
    let shared: HashSet<String> = counts
        .into_iter()
        .filter_map(|(n, c)| (c > 1).then_some(n))
        .collect();
    let globals = &Frames {
        global_names,
        chains: &chains,
        shared: &shared,
    };
    let mut converted: Vec<String> = Vec::new();
    for stmt in top.iter_mut() {
        // A top-level declaration is not nested; only its inner
        // lists are offered, which is what the walk below does.
        for_each_nested_vec_mut(stmt, &mut |list| {
            convert_list(ast, list, globals, &mut converted)
        });
    }
    // Arrow bodies hang off ExprIds the stmt spine never sees (the
    // lift's second claim surface); each arrow owns an arena slot,
    // so a flat scan covers every nesting depth — the bound is read
    // each step, so a body converted above (a new slot) is scanned
    // for the declarations nested in it too.
    let mut i = 0;
    while i < ast.exprs.len() {
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] else {
            i += 1;
            continue;
        };
        let mut body = std::mem::take(body);
        convert_list(ast, &mut body, globals, &mut converted);
        for s in body.iter_mut() {
            for_each_nested_vec_mut(s, &mut |list| {
                convert_list(ast, list, globals, &mut converted)
            });
        }
        let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[i] else {
            unreachable!("arena slot changed shape mid-walk");
        };
        *slot = body;
        i += 1;
    }
    if !converted.is_empty() {
        // The baked annotations sit inside generator BODIES — a
        // converted body now lives in the arena (a self-recursive
        // `yield* down()` is inside `down` itself), so both surfaces
        // are walked.
        for stmt in top.iter_mut() {
            widen_parser_gen_anns(std::slice::from_mut(stmt), &converted);
        }
        for e in ast.exprs.iter_mut() {
            if let Expr::ArrowFn { body, .. } = e {
                widen_parser_gen_anns(body, &converted);
            }
        }
    }
    ast.stmts = top;
}

/// What a declaration's captures are judged against: the top-level
/// names, the names each declaration's surrounding frames rebind, and
/// the generator names more than one declaration spells.
struct Frames<'a> {
    global_names: &'a [String],
    chains: &'a HashMap<DeclKey, Vec<String>>,
    shared: &'a HashSet<String>,
}

/// Count every generator declaration's name in `list`, at any depth.
fn count_gen_decls(list: &[Stmt], counts: &mut HashMap<String, usize>) {
    for s in list {
        if let Stmt::FnDecl {
            name,
            is_generator: true,
            ..
        } = s
        {
            *counts.entry(name.clone()).or_default() += 1;
        }
        for_each_nested_list(s, &mut |inner| count_gen_decls(inner, counts));
    }
}

/// Convert one list's capturing generator declarations, then move each
/// converted binding up to the first slot below every declaration of a
/// name it captures. §10.2.11 initializes a function declaration on
/// entry to its scope, so a call written above the declaration must
/// find the binding live; the wrapper closure reads a snapshot capture
/// when it is created, so it cannot rise above the declaration of
/// anything it captures, and stays where it is when that declaration
/// is right above it.
fn convert_list(
    ast: &mut Ast,
    list: &mut Vec<Stmt>,
    globals: &Frames<'_>,
    converted: &mut Vec<String>,
) {
    for i in 0..list.len() {
        let Some((name, captures)) = convert_slot(ast, &mut list[i], globals) else {
            continue;
        };
        converted.push(name);
        raise_binding(list, i, &captures);
    }
}

/// Move the binding at `list[i]` up to the first slot below every
/// declaration of a name in `captures`.
pub(super) fn raise_binding(list: &mut [Stmt], i: usize, captures: &[String]) {
    let floor = list[..i]
        .iter()
        .rposition(|s| declares_any(s, captures))
        .map_or(0, |p| p + 1);
    list[floor..=i].rotate_right(1);
}

/// Does `s` declare any of `names` in the list that holds it — a
/// lexical, function or class declaration, or a `var` however deep?
fn declares_any(s: &Stmt, names: &[String]) -> bool {
    let mut declared = Vec::new();
    list_names(std::slice::from_ref(s), &mut declared);
    hoist_var_names(std::slice::from_ref(s), &mut declared);
    declared.iter().any(|d| names.contains(d))
}

/// One slot: a nested generator declaration that captures becomes a
/// marked generator expression. Answers the converted name and what
/// it captures.
fn convert_slot(
    ast: &mut Ast,
    slot: &mut Stmt,
    frames: &Frames<'_>,
) -> Option<(String, Vec<String>)> {
    let Stmt::FnDecl {
        name,
        type_params,
        params,
        body,
        is_generator: true,
        span,
        ..
    } = &*slot
    else {
        return None;
    };
    // A generic generator has no expression form to take; the lift's
    // own mangled products are not user nesting. An ASYNC generator
    // declaration converts like a sync one: the lift has no capture
    // lane, and the expression lane is the one an async body's nested
    // async generator already takes (`desugar_async_nested_fns`), where
    // the `using`-in-asyncgeneratorbody cases pass.
    if !type_params.is_empty() || name.starts_with("__nestedgen_") {
        return None;
    }
    let shadow: &[String] = frames
        .chains
        .get(&(name.clone(), span.start, span.end))
        .map_or(&[], |c| c.as_slice());
    let narrowed = not_rebound(frames.global_names, shadow);
    let captures =
        super::free_vars::free_vars_of_arrow(ast, params, body, &narrowed, shadow, Some(name));
    if captures.is_empty() {
        return None;
    }
    let taken = std::mem::replace(slot, Stmt::Block(Vec::new()));
    let Stmt::FnDecl {
        name,
        params,
        return_type,
        body,
        span,
        ..
    } = taken
    else {
        unreachable!("matched a FnDecl above");
    };
    let eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type,
        body,
    });
    ast.set_expr_span(eid, span);
    // The decl-keyed tables move to the expression: the async channel
    // (the hoist re-registers it under the hoisted name — left keyed
    // by THIS name it would tell the let-bound wrapper apart as an
    // async-generator factory) and the destructuring-param prefix.
    // Both are keyed by name, so a name another generator declaration
    // also spells keeps its rows for that one (the lift reads them);
    // the prefix is read from this declaration's own row.
    let shared = frames.shared.contains(&name);
    let kind = if shared {
        ast.async_generator_fns.contains(&name)
    } else {
        ast.async_generator_fns.remove(&name)
    };
    let kind = if kind {
        GenFnExprKind::AsyncGenerator
    } else {
        GenFnExprKind::Generator
    };
    if !shared {
        ast.gen_param_destr_prefix.remove(&name);
    }
    let destr_prefix = ast
        .gen_param_destr_prefix_at
        .get(&(name.clone(), span.start, span.end))
        .copied()
        .unwrap_or(0);
    ast.gen_fn_exprs
        .insert(eid, GenFnExprInfo { kind, destr_prefix });
    // §15.5.5 — the declaration's own name binds inside its body
    // (self-recursion), which is what the self-name route gives an
    // expression.
    ast.fn_expr_self_names.insert(eid, name.clone());
    *slot = Stmt::LetDecl {
        mutable: false,
        name: name.clone(),
        type_ann: None,
        init: eid,
        is_var: false,
    };
    Some((name, captures))
}

/// The parser's `yield*` fast path spelled these names'
/// state-machine types; widen them to the dynamic tier — in `list`
/// itself and every list nested under it.
pub(super) fn widen_parser_gen_anns(list: &mut [Stmt], converted: &[String]) {
    let widen = |st: &mut Stmt| {
        let Stmt::LetDecl {
            type_ann: Some(ann),
            ..
        } = st
        else {
            return;
        };
        if converted
            .iter()
            .any(|n| *ann == format!("__Gen_{n}") || *ann == format!("__step_{n}"))
        {
            *ann = "any".to_string();
        }
    };
    for st in list.iter_mut() {
        widen(st);
        for_each_nested_vec_mut(st, &mut |inner| inner.iter_mut().for_each(widen));
    }
}
