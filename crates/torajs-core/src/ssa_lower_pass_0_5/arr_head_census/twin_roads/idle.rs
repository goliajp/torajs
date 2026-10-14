//! Which twin bodies never run at all.
//!
//! [`super::unreachable_twin_exprs`] answers for the reads a twin makes:
//! entered with its own class's receiver, a twin reads what the mono
//! reads. Its writes are another matter. The boxed adapter also hands
//! a twin a call whose arguments its typed face cannot take
//! (`ssa_lower_boxed_entry::build_reroute`), and then the twin stores
//! values the mono's types rule out, onto a receiver of the class
//! itself. So a twin's writes stay out of a census only when no road
//! hands it a foreign receiver and no dynamic call hands it such
//! arguments. A typed call binds the mono statically; a dynamic one is
//! a call through a receiver no class type names, which may pass
//! anything, or one of the iterator-protocol helpers' `next()` /
//! `return()`, which pass nothing and call a method only on the
//! iterator they read it from (so they are no road). A mono whose
//! parameters take every argument, or every missing one, sends no such
//! call to its twin, unless it answers for one argument count alone.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::check::Type;

/// The expressions of every twin body that never runs.
pub(crate) fn idle_twin_exprs(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> HashSet<ExprId> {
    let Some(twinned) = super::twinned(ast).filter(|t| !t.is_empty()) else {
        return HashSet::new();
    };
    let methods: HashSet<String> = twinned.iter().map(|(_, m)| m.clone()).collect();
    let twins: HashSet<&str> = ast.cmany_twins.values().map(String::as_str).collect();
    let helpers: HashSet<&str> = crate::ast::desugar_generators_forof_helpers::HELPERS.into();
    let (mut inside, mut protocol) = (HashSet::new(), HashSet::new());
    for s in &ast.stmts {
        super::collect_twin_bodies(ast, s, &twins, &mut inside);
        super::collect_twin_bodies(ast, s, &helpers, &mut protocol);
    }
    let callees = super::callees(ast);
    // the names a dynamic call may invoke with arguments, and with none
    let mut any_args: HashSet<&str> = HashSet::new();
    let mut no_args: HashSet<&str> = HashSet::new();
    if !protocol.is_empty() {
        no_args.extend(["next", "return"]);
    }
    for (i, e) in ast.exprs.iter().enumerate() {
        let id = ExprId(i as u32);
        if inside.contains(&id) || protocol.contains(&id) {
            continue;
        }
        if super::is_road(e, id, (&methods, &callees), expr_types, true) {
            return HashSet::new();
        }
        let (Expr::Call { callee, args } | Expr::OptCall { callee, args }) = e else {
            continue;
        };
        if let Expr::Member { obj, name } | Expr::OptChain { obj, name } = ast.get_expr(*callee)
            && !class_receiver(expr_types.get(obj))
        {
            match args.is_empty() {
                true => no_args.insert(name.as_str()),
                false => any_args.insert(name.as_str()),
            };
        }
    }
    let monos = mono_params(ast);
    let mut idle: HashSet<&str> = HashSet::new();
    for (mono, method) in &twinned {
        let params = monos.get(mono).copied();
        let fixed = ast.cm_fixed_argc.contains_key(*mono);
        let refuses =
            |fits: fn(&Param) -> bool| fixed || params.is_none_or(|ps| !ps.iter().all(fits));
        let runs = (any_args.contains(method.as_str()) && refuses(takes_any))
            || (no_args.contains(method.as_str()) && refuses(takes_none));
        if runs {
            continue;
        }
        if let Some(twin) = ast.cmany_twins.get(*mono) {
            idle.insert(twin.as_str());
        }
    }
    let mut out = HashSet::new();
    if !idle.is_empty() {
        for s in &ast.stmts {
            super::collect_twin_bodies(ast, s, &idle, &mut out);
        }
    }
    out
}

/// A receiver whose method calls the lowering binds statically.
fn class_receiver(t: Option<&Type>) -> bool {
    match t {
        Some(Type::ClassRef(_) | Type::Struct(_)) => true,
        Some(Type::Nullable(t)) => class_receiver(Some(t)),
        _ => false,
    }
}

/// A head parameter, or one whose slot binds every argument as it is.
fn takes_any(p: &Param) -> bool {
    matches!(p.name.as_str(), "__this" | "__new_target")
        || matches!(p.type_ann.as_deref(), Some("any" | "unknown" | "any[]"))
}

/// A parameter a missing argument leaves holding a value of its type:
/// a default fills it, or the type admits `undefined`.
fn takes_none(p: &Param) -> bool {
    takes_any(p)
        || p.default.is_some()
        || p.is_rest
        || (p.type_ann.as_deref()).is_some_and(|a| a.starts_with("__nullable(") || a == "undefined")
}

/// The parameters of every mono method with a twin, by name.
fn mono_params(ast: &Ast) -> HashMap<&str, &[Param]> {
    let mut out = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { name, params, .. } if ast.cmany_twins.contains_key(name) => {
                out.insert(name.as_str(), params.as_slice());
            }
            Stmt::Multi(v) | Stmt::Block(v) => stack.extend(v),
            _ => {}
        }
    }
    out
}
