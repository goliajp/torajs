//! The PROTOTYPE-SOURCE leg of the objlit any-lane family — a literal
//! the program installs as somebody else's [[Prototype]].
//!
//! A nominal method's body takes `__this: __ObjLit_n` and reads the
//! receiver at struct offsets. That is sound exactly while the only
//! receiver it can ever see is the literal it was written in. Putting
//! the literal on a prototype chain breaks that promise in the widest
//! possible way: every object that inherits from it calls those
//! methods with ITSELF as receiver, and nothing about the inheriting
//! object has to share the literal's layout.
//!
//! Measured before this leg (no `super` involved — the plain
//! inherited call):
//!
//! ```text
//! var parent = { getThis: function () { return this; } };
//! var a = { k: 1 };
//! Object.setPrototypeOf(a, parent);
//! a.getThis() === a        // tr: false      bun: true
//! ```
//!
//! The receiver arrives boxed, the nominal face unboxes it as a
//! struct, and the identity is gone — silently, with no diagnostic.
//!
//! Positions that install a prototype:
//!
//!   * `Object.setPrototypeOf(x, o)` / `Reflect.setPrototypeOf(x, o)`
//!     — argument 1;
//!   * `Object.create(o, …)` — argument 0;
//!   * `{ __proto__: o }` — §B.3.1 makes the field a [[Prototype]]
//!     set, which is why the anylane (h) leg already routes the
//!     CONTAINING literal to the dynobj lane; this leg is about the
//!     literal on the other side of the colon.
//!
//! WIDENING, not marking — the distinction
//! `widen_detached_method_objlits` states: marking alone gives the
//! method an `any` receiver while ordinary call sites still hand it a
//! struct pointer, which is worse than the bug. The annotation is
//! what the user could have written, and every existing lane follows
//! from it.
//!
//! Name-keyed, the same trade every leg in this family takes: a
//! shadowed binding of the same name in another scope widens too,
//! which costs the dynobj lane and never costs correctness.

use std::collections::HashSet;

use super::objlit_nominal_anylane::widen_inner;
use super::{Expr, ExprId, Stmt};

pub(super) fn widen_prototype_source_objlits(stmts: &mut [Stmt], exprs: &mut Vec<Expr>) {
    let names = prototype_source_names(exprs);
    if names.is_empty() {
        return;
    }
    let admit = |name: &str, init: ExprId| names.contains(name) && literal_has_method(exprs, init);
    widen_inner(stmts, &admit);
}

/// Binding names standing in a prototype-source position. Only a bare
/// `Ident` counts — an inline literal at the same spot is already a
/// root of the anylane collector's own legs.
fn prototype_source_names(exprs: &[Expr]) -> HashSet<String> {
    let mut out = HashSet::new();
    let note = |eid: &ExprId, out: &mut HashSet<String>| {
        if let Expr::Ident(n) = &exprs[eid.0 as usize] {
            out.insert(n.clone());
        }
    };
    for e in exprs {
        match e {
            Expr::Call { callee, args } => {
                let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
                    continue;
                };
                if !matches!(&exprs[obj.0 as usize], Expr::Ident(ns) if ns == "Object" || ns == "Reflect")
                {
                    continue;
                }
                match name.as_str() {
                    "setPrototypeOf" => {
                        if let Some(a) = args.get(1) {
                            note(a, &mut out);
                        }
                    }
                    "create" => {
                        if let Some(a) = args.first() {
                            note(a, &mut out);
                        }
                    }
                    _ => {}
                }
            }
            Expr::ObjectLit { fields } => {
                for (f, fe) in fields {
                    if f == "__proto__" {
                        note(fe, &mut out);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Does the literal carry a method the widen would be about? At this
/// stage `lift_arrow_fns` has already turned every method shorthand
/// into a `Closure`, which is the same shape the detached-method
/// leg's admit reads.
fn literal_has_method(exprs: &[Expr], init: ExprId) -> bool {
    let Expr::ObjectLit { fields } = &exprs[init.0 as usize] else {
        return false;
    };
    fields
        .iter()
        .any(|(_, fe)| matches!(&exprs[fe.0 as usize], Expr::Closure { .. }))
}
