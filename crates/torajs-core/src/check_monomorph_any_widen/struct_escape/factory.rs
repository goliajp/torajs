//! The all-`any` twin of a class factory reached through a value.
//!
//! `const KK: any = K; new KK(v)` constructs through the factory's
//! boxed adapter (`__torajs_anyv_construct`), which unboxes `v` for the
//! typed factory: a slot typed by a class takes the cell as that class,
//! and the inlined constructor calls the class's own methods on it and
//! answers `instanceof` by its tag. For a `v` of another class that is
//! a static call of a method `v` does not have. §10.2.1.3 binds the
//! argument as it is, and §13.3.2.1 / §13.10.2 read `p.m` and
//! `p instanceof P` off that value, so the factory takes the twin a
//! relay shell takes: `__new_<C>$$anyws`, the factory with every
//! non-rest parameter `any` and its return `any`. Its constructor call
//! hands those `any` arguments to the constructor, which the direct-call
//! wedge widens in turn (`check_type_of_call::any_widen`). The mono
//! adapter tests each slot and hands a call that does not fit to the
//! twin's adapter, on the same head-less face
//! (`ssa_lower_boxed_entry::twin_adapter_of`).
//!
//! Only a factory that gets a boxed adapter at all takes one: the
//! construct channel is its only caller. Out of reach, and left as
//! they are: a factory [`widenable_target`] refuses (a generic class,
//! whose factory gets no boxed adapter either, or an argv channel on
//! the factory itself).

use std::collections::{HashMap, VecDeque};

use super::closure::{all_any, inner_suffix};
use super::{twin_name, widenable_target};
use crate::ast::desugar_with::walk::{expr_children, stmt_exprs};
use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::check::{Checker, Type};
use crate::check_monomorph::WorkItem;
use crate::check_monomorph_any_widen::{RetFace, WidenTarget, emit_widened_spec};
use crate::ssa_lower_boxed_entry::{
    factory_always_admitted, program_constructs_from_value, promise_heir_classes,
};
use crate::ssa_lower_generics_monomorph::Generics;

/// Mint the twin of every class factory the construct channel reaches
/// whose constructor declares a slot the boxed adapter cannot hand
/// every value as it is.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    c: &mut Checker,
    owned_ast: &mut Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    call_retargets: &mut HashMap<ExprId, String>,
    mono_decls: &mut Vec<Stmt>,
) {
    let from_value = program_constructs_from_value(owned_ast);
    let heirs = promise_heir_classes(owned_ast);
    let factories: Vec<String> = owned_ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } if !name.contains("$$") => name
                .strip_prefix("__new_")
                .filter(|cname| from_value || factory_always_admitted(owned_ast, &heirs, cname))
                .map(|_| name.clone()),
            _ => None,
        })
        .collect();
    for factory in factories {
        let twin = twin_name(&factory);
        if c.globals.contains_key(&twin) || !c.globals.contains_key(&factory) {
            continue;
        }
        let Some(found) = widenable_target(c, owned_ast, &factory, false) else {
            continue;
        };
        let emitted = mono_decls.len();
        emit_widened_spec(
            c,
            owned_ast,
            generics,
            cache,
            worklist,
            call_retargets,
            mono_decls,
            &factory,
            &twin,
            &inner_suffix(&factory),
            &all_any(&found.params),
            RetFace::Any,
        );
        if mono_decls.len() > emitted {
            widen_ctor_scalars(c, owned_ast, &mono_decls[emitted..], &factory);
        }
    }
}

/// The twin's constructor call hands the constructor `any` arguments.
/// The direct-call wedge widens the constructor's heap slots for them
/// and coerces its `number` / `boolean` slots at the call, which would
/// bind `"7"` as the number 7 (§10.2.1.3 binds the argument as it is):
/// widen those slots on the same site, so the constructor clone it
/// calls takes every argument as passed.
fn widen_ctor_scalars(c: &mut Checker, ast: &Ast, twin: &[Stmt], factory: &str) {
    let ctor = format!("__cm_{}__ctor", &factory["__new_".len()..]);
    // a class with no constructor of its own has nothing to widen
    let Some(Type::Function(ptys, _)) = c.globals.get(&ctor) else {
        return;
    };
    let mut call = None;
    crate::ast::walk_stmts(twin, &mut |s| {
        let mut work = stmt_exprs(s);
        while let Some(e) = work.pop() {
            match ast.get_expr(e) {
                Expr::Call { callee, args } if matches!(ast.get_expr(*callee), Expr::Ident(n) if *n == ctor) =>
                {
                    call = Some((e, args.clone()));
                }
                _ => work.extend(expr_children(ast, e)),
            }
        }
    });
    let (eid, args) = call.expect("a factory twin calls its class's constructor");
    let scalars: Vec<usize> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            matches!(ast.get_expr(**a), Expr::Ident(_))
                && matches!(ptys.get(*i), Some(Type::Number | Type::Boolean))
                // a widened clone binds a run-time `undefined` as is,
                // where the typed body would take the default
                && !crate::check_type_of_call::any_widen::param_has_default(ast, &ctor, *i)
        })
        .map(|(i, _)| i)
        .collect();
    if scalars.is_empty() {
        return;
    }
    let site = c
        .any_widen_mono_sites
        .entry(eid)
        .or_insert_with(|| (ctor.clone(), Vec::new()));
    for i in scalars {
        if !site.1.iter().any(|(j, _)| *j == i) {
            site.1.push((i, WidenTarget::Scalar));
        }
    }
}
