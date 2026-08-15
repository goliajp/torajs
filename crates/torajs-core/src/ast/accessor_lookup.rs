//! Parent-chain accessor lookup — rotation 413 blade 2.
//!
//! `ast.accessor_getters` / `ast.accessor_setters` record each pair
//! under the DECLARING class only, so `class B extends A {}` probing
//! `("B", "x")` missed an accessor A declared and fell through to the
//! any-member lane: the read still answered right at runtime (the
//! §10.1.7 prototype walk hits the reified AccessorPair) but the
//! checker typed it `Any` and the lowering paid a dynamic dispatch.
//! These walks resolve the pair the way [[Get]]/[[Set]] would — own
//! class first, then up `class_parents` — so an inherited accessor
//! keeps its declared type and its direct call. Hop-capped by the
//! class count (heritage cycles are rejected upstream; the cap keeps
//! a malformed table finite).

use std::collections::HashMap;

use super::Ast;

pub(crate) fn accessor_getter_in_chain(ast: &Ast, cls: &str, prop: &str) -> Option<String> {
    accessor_in_chain(&ast.accessor_getters, &ast.class_parents, cls, prop)
}

pub(crate) fn accessor_setter_in_chain(ast: &Ast, cls: &str, prop: &str) -> Option<String> {
    accessor_in_chain(&ast.accessor_setters, &ast.class_parents, cls, prop)
}

fn accessor_in_chain(
    table: &HashMap<(String, String), String>,
    class_parents: &HashMap<String, Option<String>>,
    cls: &str,
    prop: &str,
) -> Option<String> {
    let mut cur = Some(cls.to_string());
    for _ in 0..=class_parents.len() {
        let c = cur?;
        if let Some(f) = table.get(&(c.clone(), prop.to_string())) {
            return Some(f.clone());
        }
        cur = class_parents.get(&c).cloned().flatten();
    }
    None
}
