//! A class's construct factory (`__new_<C>`) carries copies of its
//! constructor's parameters, defaults included, and hands them to
//! `__cm_<C>__ctor` unchanged. When the constructor guards a default
//! itself, the factory must not guard the same parameter as well:
//! the factory's guard runs before the instance's field initializers
//! (§10.2.2 initializes fields before FunctionDeclarationInstantiation
//! evaluates the defaults), and the constructor's `arguments` would
//! then see the filled-in value where §10.4.4 wants the one the
//! caller passed. Such a factory parameter relays: it becomes an
//! `any` slot padded with `undefined`, with no guard.

use super::{Ast, Conv, Stmt};
use std::collections::{HashMap, HashSet};

/// Replace every factory entry of `sites` (`(stmt index, param index,
/// param name, plan)`) whose constructor guards the same-named
/// parameter with a [`Conv::Relay`].
pub(super) fn relay_factory_params(ast: &Ast, sites: &mut Vec<(usize, usize, String, Conv)>) {
    let mut guarded: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (si, _, pname, _) in sites.iter() {
        if let Stmt::FnDecl { name, .. } = &ast.stmts[*si]
            && let Some(cname) = ctor_class(name)
        {
            guarded.entry(cname).or_default().insert(pname.as_str());
        }
    }
    if guarded.is_empty() {
        return;
    }
    let mut relays: Vec<(usize, usize, String)> = Vec::new();
    for (si, s) in ast.stmts.iter().enumerate() {
        let Stmt::FnDecl { name, params, .. } = s else {
            continue;
        };
        let Some(names) = name.strip_prefix("__new_").and_then(|c| guarded.get(c)) else {
            continue;
        };
        for (pi, p) in params.iter().enumerate() {
            if p.default.is_some() && names.contains(p.name.as_str()) {
                relays.push((si, pi, p.name.clone()));
            }
        }
    }
    sites.retain(|(si, pi, _, _)| !relays.iter().any(|(rs, rp, _)| rs == si && rp == pi));
    sites.extend(
        relays
            .into_iter()
            .map(|(si, pi, n)| (si, pi, n, Conv::Relay)),
    );
}

fn ctor_class(name: &str) -> Option<&str> {
    name.strip_prefix("__cm_")?.strip_suffix("__ctor")
}

/// Where a FnDecl's guards splice in. A base class constructor's go
/// past its field initializers (the recorded end); a derived one's go
/// at the head, ahead of the `super(…)` call those initializers
/// follow — the defaults are evaluated before any of the body runs,
/// though after the locals the class desugar declares there (the
/// receiver, the parent's answer, the this-binding flag) since a
/// default may read `this`. Every other function takes the head.
pub(super) fn guard_offset(ast: &Ast, name: &str, body: &[Stmt]) -> usize {
    let Some(cname) = ctor_class(name) else {
        return 0;
    };
    if ast.class_parents.get(cname).is_some_and(Option::is_some) {
        let frame = [
            "__this",
            "__sup",
            crate::ast::desugar_classes_this_tdz::FLAG,
        ];
        return body
            .iter()
            .take_while(
                |s| matches!(s, Stmt::LetDecl { name, .. } if frame.contains(&name.as_str())),
            )
            .count();
    }
    ast.ctor_field_init_end.get(cname).copied().unwrap_or(0)
}
