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
//! `let <name> = function* (…) { … }` in the slot it occupied, marked
//! in `gen_fn_exprs` exactly as the parser marks a written generator
//! expression — and [`super::hoist_gen_fn_exprs`], which runs right
//! after this, gives it the wrapper route: captured names become
//! call-time snapshot params, a reassigned capture stays refused loud.
//! Whatever this leaves behind reaches the lift unchanged.
//!
//! The rewrite stays where the declaration was, for the reason the
//! nested-fn sibling gives: hoisting only buys using the name before
//! its declaration, and a declaration with no captures never leaves
//! the lift lane, where hoisting already works.
//!
//! The parser's `yield* g()` fast path bakes `__Gen_g` / `__step_g`
//! annotations at parse time; a converted `g` mints no such class,
//! so those annotations widen to `any` here — the same remap the
//! lift does for its mangled names, aimed at the dynamic tier instead.

use super::scope_chain::{DeclKey, chains_by_fn_decl, not_rebound};
use super::stmt_nested_lists::for_each_nested_vec_mut;
use super::{Ast, Expr, GenFnExprInfo, GenFnExprKind, Stmt};
use std::collections::HashMap;

/// Convert every capturing nested generator declaration in place.
/// `global_names` is the top-level binding set the expression hoist
/// pre-binds — a name in it is not a capture, unless a frame around
/// the declaration rebinds it (`scope_chain`).
pub(super) fn route_capturing_nested_gen_decls(ast: &mut Ast, global_names: &[String]) {
    let mut top = std::mem::take(&mut ast.stmts);
    let chains = chains_by_fn_decl(ast, &[&top]);
    let globals = (global_names, &chains);
    let mut converted: Vec<String> = Vec::new();
    for stmt in top.iter_mut() {
        // A top-level declaration is not nested; only its inner
        // lists are offered, which is what the walk below does.
        for_each_nested_vec_mut(stmt, &mut |list| {
            for slot in list.iter_mut() {
                if let Some(name) = convert_slot(ast, slot, globals) {
                    converted.push(name);
                }
            }
        });
    }
    // Arrow bodies hang off ExprIds the stmt spine never sees (the
    // lift's second claim surface); each arrow owns an arena slot,
    // so a flat scan covers every nesting depth.
    for i in 0..ast.exprs.len() {
        let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] else {
            continue;
        };
        let mut body = std::mem::take(body);
        for s in body.iter_mut() {
            if let Some(name) = convert_slot(ast, s, globals) {
                converted.push(name);
            }
            for_each_nested_vec_mut(s, &mut |list| {
                for slot in list.iter_mut() {
                    if let Some(name) = convert_slot(ast, slot, globals) {
                        converted.push(name);
                    }
                }
            });
        }
        let Expr::ArrowFn { body: slot, .. } = &mut ast.exprs[i] else {
            unreachable!("arena slot changed shape mid-walk");
        };
        *slot = body;
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

/// One slot: a nested generator declaration that captures becomes a
/// marked generator expression. Answers the converted name.
fn convert_slot(
    ast: &mut Ast,
    slot: &mut Stmt,
    (global_names, chains): (&[String], &HashMap<DeclKey, Vec<String>>),
) -> Option<String> {
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
    // declaration stays on the lift lane too (rotation 634 close):
    // converted, `using _ = r; yield; await pending` disposed `r`
    // during the await where the lifted form keeps it alive — the
    // two lanes disagree about that suspension, and the sweep moved
    // three passing `using`-in-asyncgeneratorbody cases to red.
    // Which lane is right is open (the expression lane matches a
    // TOP-LEVEL async generator declaration's answer, and that one
    // is wrong against bun); until it is settled the async form
    // keeps the behaviour it had.
    if !type_params.is_empty()
        || name.starts_with("__nestedgen_")
        || ast.async_generator_fns.contains(name)
    {
        return None;
    }
    let shadow: &[String] = chains
        .get(&(name.clone(), span.start, span.end))
        .map_or(&[], |c| c.as_slice());
    let narrowed = not_rebound(global_names, shadow);
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
    let kind = if ast.async_generator_fns.remove(&name) {
        GenFnExprKind::AsyncGenerator
    } else {
        GenFnExprKind::Generator
    };
    let destr_prefix = ast.gen_param_destr_prefix.remove(&name).unwrap_or(0);
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
    Some(name)
}

/// The parser's `yield*` fast path spelled the converted name's
/// state-machine types; widen them to the dynamic tier — in `list`
/// itself and every list nested under it.
fn widen_parser_gen_anns(list: &mut [Stmt], converted: &[String]) {
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
