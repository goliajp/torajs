//! Parent-chain accessor walks.
//!
//! `ast.accessor_getters` / `ast.accessor_setters` record each pair
//! under the DECLARING class only, so these walks resolve the pair the
//! way [[Get]] / [[Set]] would — own class first, then up
//! `class_parents` — and an inherited accessor keeps its declared type
//! and its direct call.
//!
//! The walk starts from a class KEY that may be a generic
//! instantiation (`"Box<number>"`) and composes the written heritage
//! arguments per hop (`class_parent_type_args` × `class_type_params`),
//! so a hit on a generic declarer carries the substitution that maps
//! its type-param names to spellings resolvable at the receiver.
//! Hop-capped by the class count (heritage cycles are rejected
//! upstream; the cap keeps a malformed table finite).

use std::collections::HashMap;

use super::super::{Ast, PropKey};
use super::AccessorHit;
use crate::check_type_ann::split_top_pipe;
use crate::check_type_ann_substitute::ann_substitute;

type Subst = Vec<(String, String)>;

/// The getter a read of `prop` through a receiver statically typed
/// `cls_key` runs, when one function or one vtable slot answers for
/// every instance that type admits. `None` sends the site to the
/// any-member lane, whose runtime walk settles it: no accessor at all,
/// a pair that lacks the getter (§10.1.8.1 answers undefined), or a
/// subclass shadowing the pair in a way no slot carries
/// ([`face_varies_below`]).
pub(crate) fn accessor_getter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    let faces = Faces {
        this: &ast.accessor_getters,
        other: &ast.accessor_setters,
        suffix: "_get",
    };
    accessor_in_chain(&faces, ast, cls_key, prop, true)
}

/// The setter twin of [`accessor_getter_in_chain`]; a pair that lacks
/// the setter refuses the write at runtime (§10.1.9.2 step 7) in the
/// writer's strictness.
pub(crate) fn accessor_setter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    accessor_in_chain(&setter_faces(ast), ast, cls_key, prop, true)
}

/// The setter the static type `cls_key` declares for `prop`, whatever
/// a subclass redeclares: what the checker holds a write to. A value
/// that setter refuses is a type error whichever instance arrives at
/// runtime; the lowering asks [`accessor_setter_in_chain`].
pub(crate) fn declared_setter_in_chain(
    ast: &Ast,
    cls_key: &str,
    prop: &str,
) -> Option<AccessorHit> {
    accessor_in_chain(&setter_faces(ast), ast, cls_key, prop, false)
}

fn setter_faces(ast: &Ast) -> Faces<'_> {
    Faces {
        this: &ast.accessor_setters,
        other: &ast.accessor_getters,
        suffix: "_set",
    }
}

/// Whether reading `prop` through a receiver statically typed
/// `cls_key` may run a getter. Unlike [`accessor_getter_in_chain`],
/// which answers only when one function or slot covers every
/// instance, this also says yes when the answer depends on the
/// instance: any class at or above the static class, or below it,
/// that declares a getter for `prop` may be the one that runs.
pub(crate) fn getter_may_run(ast: &Ast, cls_key: &str, prop: &str) -> bool {
    let head = cls_key.split('<').next().unwrap_or(cls_key);
    let key = PropKey::from(prop);
    let parents = &ast.class_parents;
    ast.accessor_getters.keys().any(|(c, p)| {
        *p == key
            && (super::super::method_owner_is_in_chain(parents, c, head)
                || super::super::method_owner_is_in_chain(parents, head, c))
    })
}

/// One face of the accessor tables, read alongside the other. A class
/// declaring either face owns the whole property (one accessor
/// property per key on its prototype), so the walk stops there either
/// way.
struct Faces<'a> {
    this: &'a HashMap<(String, PropKey), String>,
    other: &'a HashMap<(String, PropKey), String>,
    suffix: &'static str,
}

/// Whether a strict descendant of `head` redeclares `prop` in a way
/// this face's vtable slot does not carry: the descendant's pair lacks
/// the face, or no accessor slot of that face lists it (a regular
/// method spelled like the face's symbol, or a face reading
/// `arguments`, keeps the accessor out of the slots). The function a
/// read or write runs then depends on the instance in a way only the
/// runtime walk knows.
fn face_varies_below(faces: &Faces<'_>, ast: &Ast, head: &str, key: &PropKey) -> bool {
    let mut below = faces
        .this
        .keys()
        .chain(faces.other.keys())
        .filter(|(c, p)| p == key && c != head)
        .filter(|(c, _)| super::super::method_owner_is_in_chain(&ast.class_parents, head, c))
        .peekable();
    if below.peek().is_none() {
        return false;
    }
    let slot = format!("{}{}", super::super::mangle_key(key), faces.suffix);
    let owners = (ast.accessor_slots.contains(&slot) && ast.method_index.contains_key(&slot))
        .then(|| ast.method_owners.get(&slot))
        .flatten();
    below.any(|(c, _)| {
        !faces.this.contains_key(&(c.clone(), key.clone()))
            || !owners.is_some_and(|o| o.contains(c))
    })
}

fn accessor_in_chain(
    faces: &Faces<'_>,
    ast: &Ast,
    cls_key: &str,
    prop: &str,
    per_instance: bool,
) -> Option<AccessorHit> {
    let (head, mut subst) = seed_subst(ast, cls_key)?;
    let prop_key = PropKey::from(prop);
    let mut cur = Some(head.to_string());
    for _ in 0..=ast.class_parents.len() {
        let c = cur?;
        let key = (c.clone(), prop_key.clone());
        if let Some(f) = faces.this.get(&key) {
            if per_instance && face_varies_below(faces, ast, head, &prop_key) {
                return None;
            }
            return Some(AccessorHit {
                fn_name: f.clone(),
                subst,
            });
        }
        if faces.other.contains_key(&key) {
            return None;
        }
        let parent = ast.class_parents.get(&c).cloned().flatten();
        if let Some(p) = &parent {
            subst = parent_subst(ast, &c, p, &subst);
        }
        cur = parent;
    }
    None
}

/// The walk's starting class and substitution: a generic-instantiation
/// key seeds it from its own written arguments; a bare class name
/// starts empty.
fn seed_subst<'k>(ast: &Ast, cls_key: &'k str) -> Option<(&'k str, Subst)> {
    let Some(i) = cls_key.find('<').filter(|_| cls_key.ends_with('>')) else {
        return Some((cls_key, Vec::new()));
    };
    let head = &cls_key[..i];
    let args = split_top_pipe(&cls_key[i + 1..cls_key.len() - 1]);
    let tps = ast.class_type_params.get(head)?;
    let subst = tps
        .iter()
        .enumerate()
        .map(|(k, tp)| {
            let arg = args
                .get(k)
                .map_or_else(|| "any".to_string(), |s| s.to_string());
            (tp.clone(), arg)
        })
        .collect();
    Some((head, subst))
}

/// The substitution in `parent`'s vocabulary, one hop up from `c`. The
/// written heritage args are spelled in `c`'s vocabulary, so the
/// current substitution applies to them first (chains re-spelling
/// params stay concrete at the receiver).
fn parent_subst(ast: &Ast, c: &str, parent: &str, subst: &Subst) -> Subst {
    let Some(ptp) = ast.class_type_params.get(parent).filter(|t| !t.is_empty()) else {
        return Vec::new();
    };
    let written = ast.class_parent_type_args.get(c);
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
                ann_substitute(&raw, subst)
            };
            (tp.clone(), arg)
        })
        .collect()
}
