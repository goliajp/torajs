//! `interface B extends A, C<T> { … }` — the members B inherits.
//!
//! The parser records each heritage type as a leading
//! `(EXTENDS_FIELD, <ann>)` row of the interface's field list. This
//! pass, run before anything reads a `TypeDecl`, replaces those rows
//! with the parent's own members (its heritage already resolved,
//! its type parameters replaced by the written arguments), parent
//! members first, then the interface's own; an own member of the
//! same name replaces the inherited one in place, as TS requires it
//! to be assignable to the inherited one anyway. A parent that is not a struct-shaped type
//! declaration in this program (a class, a builtin) contributes
//! nothing, which is what the dropped heritage did before.

use super::{Ast, PropKey, Stmt};
use std::collections::HashMap;

/// The field name a heritage row is spelled with.
pub(crate) const EXTENDS_FIELD: &str = "__extends__";

type Decl = (Vec<String>, Vec<(PropKey, String)>);

pub fn flatten_interface_extends(ast: &mut Ast) {
    let mut decls: HashMap<String, Decl> = HashMap::new();
    let mut pending = false;
    for s in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        {
            pending |= fields.iter().any(|(k, _)| is_extends(k));
            decls.insert(name.clone(), (type_params.clone(), fields.clone()));
        }
    }
    if !pending {
        return;
    }
    let mut done: HashMap<String, Vec<(PropKey, String)>> = HashMap::new();
    let mut inherited: Vec<(String, PropKey)> = Vec::new();
    let mut inherited_plain: Vec<(String, PropKey)> = Vec::new();
    for s in ast.stmts.iter_mut() {
        if let Stmt::TypeDecl { name, fields, .. } = s
            && fields.iter().any(|(k, _)| is_extends(k))
        {
            let keys = optional_keys(name, &decls, &ast.optional_members, 0);
            inherited.extend(keys.into_iter().map(|k| (name.clone(), k)));
            let plain = optional_keys(name, &decls, &ast.optional_plain_members, 0);
            inherited_plain.extend(plain.into_iter().map(|k| (name.clone(), k)));
            *fields = resolve(name, &decls, &mut done, 0);
        }
    }
    ast.optional_members.extend(inherited);
    ast.optional_plain_members.extend(inherited_plain);
}

/// The members `name` declares optional or inherits optional and does
/// not redeclare.
fn optional_keys(
    name: &str,
    decls: &HashMap<String, Decl>,
    optional: &std::collections::HashSet<(String, PropKey)>,
    depth: usize,
) -> Vec<PropKey> {
    let Some((_, fields)) = decls.get(name).filter(|_| depth <= decls.len()) else {
        return Vec::new();
    };
    let mut out: Vec<PropKey> = Vec::new();
    for (_, ann) in fields.iter().filter(|(k, _)| is_extends(k)) {
        let parent = split_instantiation(ann).0;
        for k in optional_keys(parent, decls, optional, depth + 1) {
            if !fields.iter().any(|(own, _)| *own == k) {
                out.push(k);
            }
        }
    }
    out.extend(
        fields
            .iter()
            .filter(|(k, _)| optional.contains(&(name.to_string(), k.clone())))
            .map(|(k, _)| k.clone()),
    );
    out
}

fn is_extends(k: &PropKey) -> bool {
    k.lossy() == EXTENDS_FIELD
}

/// The member list of `name` with every heritage row expanded.
fn resolve(
    name: &str,
    decls: &HashMap<String, Decl>,
    done: &mut HashMap<String, Vec<(PropKey, String)>>,
    depth: usize,
) -> Vec<(PropKey, String)> {
    if let Some(f) = done.get(name) {
        return f.clone();
    }
    let Some((_, fields)) = decls.get(name) else {
        return Vec::new();
    };
    // a heritage cycle is a TS error; stop rather than recurse forever
    if depth > decls.len() {
        return fields
            .iter()
            .filter(|(k, _)| !is_extends(k))
            .cloned()
            .collect();
    }
    let mut out: Vec<(PropKey, String)> = Vec::new();
    for (_, ann) in fields.iter().filter(|(k, _)| is_extends(k)) {
        let (parent, args) = split_instantiation(ann);
        let Some((params, _)) = decls.get(parent) else {
            continue;
        };
        let subst: Vec<(String, String)> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    p.clone(),
                    args.get(i).cloned().unwrap_or_else(|| "any".into()),
                )
            })
            .collect();
        for (pk, pann) in resolve(parent, decls, done, depth + 1) {
            let pann = if subst.is_empty() {
                pann
            } else {
                crate::check_type_ann_substitute::ann_substitute(&pann, &subst)
            };
            match out.iter_mut().find(|(k2, _)| *k2 == pk) {
                Some(slot) => slot.1 = pann,
                None => out.push((pk, pann)),
            }
        }
    }
    // a redeclared member keeps the inherited position: an object
    // typed by the child stays a prefix-compatible value of the parent
    for (k, ann) in fields.iter().filter(|(k, _)| !is_extends(k)) {
        match out.iter_mut().find(|(k2, _)| k2 == k) {
            Some(slot) => slot.1 = ann.clone(),
            None => out.push((k.clone(), ann.clone())),
        }
    }
    done.insert(name.to_string(), out.clone());
    out
}

/// `Name<A|B>` → (`Name`, [`A`, `B`]), splitting the arguments at
/// depth-0 `|` (the annotation encoding's argument separator).
fn split_instantiation(ann: &str) -> (&str, Vec<String>) {
    let Some(open) = ann.find('<') else {
        return (ann, Vec::new());
    };
    let inner = ann[open + 1..]
        .strip_suffix('>')
        .unwrap_or(&ann[open + 1..]);
    let mut args = Vec::new();
    let (mut depth, mut last, mut prev) = (0i32, 0usize, 0u8);
    for (i, &b) in inner.as_bytes().iter().enumerate() {
        match b {
            b'(' | b'<' => depth += 1,
            b'>' if prev == b'-' => {}
            b')' | b'>' => depth -= 1,
            b'|' if depth == 0 => {
                args.push(inner[last..i].to_string());
                last = i + 1;
            }
            _ => {}
        }
        prev = b;
    }
    args.push(inner[last..].to_string());
    (&ann[..open], args)
}
