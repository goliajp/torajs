//! The callback annotations a `Map` / `Set` receiver gives: the
//! `forEach` parameters and the closures `set` / `add` store.

use std::collections::HashMap;

use super::project_fn_type_ann;
use crate::ast::Ast;

/// Callback param/return annotations for `forEach` on a `Map<K|V>` /
/// `Set<T>` receiver ann (the flat generic spelling). None for any
/// other method or receiver shape — Map/Set carry no other
/// callback-bearing methods.
pub(in crate::ast) fn mapset_foreach_expected(
    ann: &str,
    method: &str,
) -> Option<(Vec<String>, String)> {
    if method != "forEach" {
        return None;
    }
    if let Some(inner) = ann.strip_prefix("Map<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner);
        let [k, v] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![v.to_string(), k.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    if let Some(inner) = ann.strip_prefix("Set<").and_then(|r| r.strip_suffix('>')) {
        let parts = crate::check_type_ann::split_top_pipe(inner);
        let [t] = parts.as_slice() else {
            return None;
        };
        return Some((
            vec![t.to_string(), t.to_string(), ann.to_string()],
            "void".into(),
        ));
    }
    None
}

/// A closure handed to `Map.prototype.set` (§24.1.3.9) or
/// `Set.prototype.add` (§24.2.3.1) is stored, not called: the key /
/// value / member type of the receiver is its type, as an element
/// type is for `push`. Left without it, an arrow over an `any` read
/// returned `any` while `forEach`'s typed callback called it through
/// the declared signature and read the box bits as the number.
/// Returns whether a closure argument was seeded.
pub(in crate::ast) fn apply_mapset_stored_arm(
    ast: &Ast,
    ann: &str,
    method: &str,
    closure_args: &[(usize, String)],
    updates: &mut HashMap<String, (Vec<String>, String)>,
) -> bool {
    // The parser spells `Map<K, V>` as `Map<K|V>`, so a union key or
    // value leaves more top-level parts than arguments and no way to
    // tell which belong to the key: such a receiver seeds nothing, as
    // `mapset_foreach_expected` answers nothing for it.
    let (prefix, arity) = match method {
        "set" => ("Map<", 2),
        "add" => ("Set<", 1),
        _ => return false,
    };
    let Some(inner) = ann.strip_prefix(prefix).and_then(|r| r.strip_suffix('>')) else {
        return false;
    };
    let anns = crate::check_type_ann::split_top_pipe(inner);
    if anns.len() != arity {
        return false;
    }
    let mut seeded = false;
    for (arg_idx, fn_name) in closure_args {
        if let Some(projected) = anns.get(*arg_idx).and_then(|a| project_fn_type_ann(ast, a)) {
            updates.insert(fn_name.clone(), projected);
            seeded = true;
        }
    }
    seeded
}
