//! The object type literals that spell an interface whose method a
//! round retypes to return `any`.
//!
//! A literal written in place (`o: { m(): number }`) checks as the
//! declaration of its shape, the one `iface_witness::inline` makes for
//! it or one the program declares: a call through it dispatches by the
//! rows of the declaration whose type is equal to its own. Once the
//! declaration's member returns `any`, a literal still spelling the old
//! return would be equal to no declaration, and a class instance would
//! no longer be one of its values. Every literal of the declaration's
//! shape is respelled with it.

use std::collections::HashSet;

use super::inlobj_fields;
use super::method_rets::{Family, returning_any};
use crate::ast::{Ast, Stmt};

/// A declaration's members `(name, annotation)` in name order, and the
/// names of those whose return is retyped.
type Shape = (Vec<(String, String)>, HashSet<String>);

/// Respell every object type literal of the shape of an interface with
/// a member in `fam.members`, before the interface itself is retyped.
pub(super) fn retype(r: &mut Ast, fam: &Family) {
    let shapes = shapes(r, fam);
    if shapes.is_empty() {
        return;
    }
    inlobj_fields::respell(r, &|ms| {
        let mut shape: Vec<(String, String)> = (ms.iter())
            .map(|(k, a)| (k.trim().to_string(), a.trim().to_string()))
            .collect();
        shape.sort();
        let keys: HashSet<&String> = (shapes.iter())
            .filter(|(s, _)| *s == shape)
            .flat_map(|(_, ks)| ks)
            .collect();
        if keys.is_empty() {
            return None;
        }
        let member = |&(k, a): &(&str, &str)| match returning_any(a.trim()) {
            Some(n) if keys.contains(&k.trim().to_string()) => format!("{k}:{n}"),
            _ => format!("{k}:{a}"),
        };
        Some(ms.iter().map(member).collect())
    });
}

/// The shapes of the non-generic declarations with a member in
/// `fam.members`, as `r` spells them now.
fn shapes(r: &Ast, fam: &Family) -> Vec<Shape> {
    let mut out = Vec::new();
    for s in &r.stmts {
        let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        else {
            continue;
        };
        if !type_params.is_empty() {
            continue;
        }
        let keys: HashSet<String> = (fields.iter())
            .filter(|(k, _)| fam.members.contains(&(name.clone(), k.clone())))
            .filter_map(|(k, _)| k.as_str().map(str::to_string))
            .collect();
        let shape = (fields.iter())
            .map(|(k, a)| Some((k.as_str()?.to_string(), a.trim().to_string())))
            .collect::<Option<Vec<_>>>();
        if let Some(mut shape) = shape.filter(|_| !keys.is_empty()) {
            shape.sort();
            out.push((shape, keys));
        }
    }
    out
}
