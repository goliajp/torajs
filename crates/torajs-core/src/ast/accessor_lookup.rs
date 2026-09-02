//! Parent-chain accessor lookup — rotation 413 blade 2, generic
//! substitution added in blade 3.
//!
//! `ast.accessor_getters` / `ast.accessor_setters` record each pair
//! under the DECLARING class only, so `class B extends A {}` probing
//! `("B", "x")` missed an accessor A declared and fell through to the
//! any-member lane: the read still answered right at runtime (the
//! §10.1.7 prototype walk hits the reified AccessorPair) but the
//! checker typed it `Any` and the lowering paid a dynamic dispatch.
//! These walks resolve the pair the way [[Get]]/[[Set]] would — own
//! class first, then up `class_parents` — so an inherited accessor
//! keeps its declared type and its direct call.
//!
//! The walk starts from a class KEY that may be a generic
//! instantiation (`"Box<number>"`) and composes the written heritage
//! arguments per hop (`class_parent_type_args` × `class_type_params`,
//! the `compute_full_fields` flattening recipe), so a hit on a
//! generic declarer carries the substitution that maps its type-param
//! names to spellings resolvable at the receiver. Hop-capped by the
//! class count (heritage cycles are rejected upstream; the cap keeps
//! a malformed table finite).

use std::collections::HashMap;

use super::{Ast, PropKey};
use crate::check_type_ann::split_top_pipe;
use crate::check_type_ann_substitute::ann_substitute;

pub(crate) struct AccessorHit {
    pub fn_name: String,
    /// The declaring class's type-param names zipped against the
    /// instantiation-argument spellings composed down the walk —
    /// empty for a non-generic declarer.
    pub subst: Vec<(String, String)>,
}

pub(crate) fn accessor_getter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    accessor_in_chain(&ast.accessor_getters, ast, cls_key, prop)
}

pub(crate) fn accessor_setter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    accessor_in_chain(&ast.accessor_setters, ast, cls_key, prop)
}

fn accessor_in_chain(
    table: &HashMap<(String, PropKey), String>,
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    // A generic-instantiation key seeds the substitution from its own
    // written arguments; a bare class name starts empty.
    let (head, mut subst): (&str, Vec<(String, String)>) = match cls_key.find('<') {
        Some(i) if cls_key.ends_with('>') => {
            let head = &cls_key[..i];
            let args = split_top_pipe(&cls_key[i + 1..cls_key.len() - 1]);
            let tps = ast.class_type_params.get(head)?;
            (
                head,
                tps.iter()
                    .enumerate()
                    .map(|(k, tp)| {
                        (
                            tp.clone(),
                            args.get(k)
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| "any".to_string()),
                        )
                    })
                    .collect(),
            )
        }
        _ => (cls_key, Vec::new()),
    };
    let mut cur = Some(head.to_string());
    for _ in 0..=ast.class_parents.len() {
        let c = cur?;
        if let Some(f) = table.get(&(c.clone(), PropKey::from(prop))) {
            return Some(AccessorHit {
                fn_name: f.clone(),
                subst,
            });
        }
        // Compose the substitution for the parent's vocabulary before
        // hopping — the written heritage args are spelled in `c`'s
        // vocabulary, so the current subst applies to them first
        // (chains re-spelling params stay concrete at the receiver).
        let parent = ast.class_parents.get(&c).cloned().flatten();
        if let Some(p) = &parent {
            let ptp = ast.class_type_params.get(p).cloned().unwrap_or_default();
            subst = if ptp.is_empty() {
                Vec::new()
            } else {
                let written = ast.class_parent_type_args.get(&c);
                ptp.iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let raw = written
                            .and_then(|a| a.get(i))
                            .cloned()
                            .unwrap_or_else(|| "any".to_string());
                        let arg = if subst.is_empty() {
                            raw
                        } else {
                            ann_substitute(&raw, &subst)
                        };
                        (tp.clone(), arg)
                    })
                    .collect()
            };
        }
        cur = parent;
    }
    None
}
