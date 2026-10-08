//! One class's row for an interface: how each face reaches the
//! class's own member, read off its field list and its `__cm_` bodies.

use super::super::{get_face, has_face, set_face};
use super::{Body, Decls, Member, MemberKind};
use crate::ast::Ast;

/// The class's thunk bodies for every face of the interface, or `None`
/// when some required member is missing or does not match.
pub(super) fn class_row(
    ast: &Ast,
    d: &Decls<'_>,
    eq: &crate::ast::vtable_slot::ann_eq::AnnEq<'_>,
    c: &str,
    members: &[Member],
) -> Option<Vec<(String, usize, Body)>> {
    let fields = d.class_fields[c];
    let field_ann = |k: &str| fields.iter().find(|(n, _)| *n == *k).map(|(_, a)| a);
    let mut row = Vec::new();
    for (i, m) in members.iter().enumerate() {
        match &m.kind {
            MemberKind::Method { params, ret } => {
                let (body, has, get) = if let Some(cm) = declarer(ast, d, c, &m.key) {
                    let call = method_body(ast, d, eq, &cm, params, ret)?;
                    (call, Body::HasTrue, Body::FieldGet)
                } else if let Some(a) = field_ann(&m.key) {
                    let call = eq
                        .same(a, &m.ann)
                        .then_some(Body::MemberCall(params.len()))?;
                    (call, Body::HasField, Body::FieldGet)
                } else if m.optional {
                    (Body::DynCall, Body::HasDyn, Body::DynGet)
                } else {
                    return None;
                };
                row.push((m.key.clone(), i, body));
                row.push((get_face(&m.key), i, get));
                if m.optional {
                    row.push((has_face(&m.key), i, has));
                }
            }
            MemberKind::Data => {
                let (get, set) = match field_ann(&m.key) {
                    Some(a) if eq.same(a, &m.ann) => (Body::FieldGet, Body::FieldSet),
                    Some(_) => return None,
                    None if m.optional && !has_accessor(ast, c, &m.key) => {
                        (Body::DynGet, Body::DynSet)
                    }
                    None => return None,
                };
                row.push((get_face(&m.key), i, get));
                row.push((set_face(&m.key), i, set));
            }
        }
    }
    Some(row)
}

/// The `__cm_` body `c` answers `key` with: its own, or the nearest
/// ancestor's.
fn declarer(ast: &Ast, d: &Decls<'_>, c: &str, key: &str) -> Option<String> {
    let mut cur = Some(c.to_string());
    let mut hops = ast.class_parents.len() + 1;
    while let Some(x) = cur {
        let name = format!("__cm_{x}__{key}");
        if d.fns.contains_key(name.as_str()) {
            return Some(name);
        }
        hops = hops.checked_sub(1)?;
        cur = ast.class_parents.get(&x).cloned().flatten();
    }
    None
}

/// Whether `c` (or an ancestor) declares `key` as an accessor.
fn has_accessor(ast: &Ast, c: &str, key: &str) -> bool {
    let k = crate::ast::PropKey::from(key);
    let mut cur = Some(c.to_string());
    let mut hops = ast.class_parents.len() + 1;
    while let Some(x) = cur {
        let at = (x.clone(), k.clone());
        if ast.accessor_getters.contains_key(&at) || ast.accessor_setters.contains_key(&at) {
            return true;
        }
        hops -= 1;
        if hops == 0 {
            break;
        }
        cur = ast.class_parents.get(&x).cloned().flatten();
    }
    false
}

/// How a thunk calls the method body `cm`: directly when it is the only
/// body of its name, else through the member-call lanes (which pick the
/// override for the instance). `None` when its signature does not
/// match the interface's.
fn method_body(
    ast: &Ast,
    d: &Decls<'_>,
    eq: &crate::ast::vtable_slot::ann_eq::AnnEq<'_>,
    cm: &str,
    params: &[String],
    ret: &str,
) -> Option<Body> {
    let (tps, ps, r) = d.fns[cm];
    let own = ps.get(1..)?;
    if !tps.is_empty()
        || own.len() > params.len()
        || own.iter().any(|p| p.is_rest)
        || ast.this_first_argc_fns.contains_key(cm)
    {
        return None;
    }
    for (p, want) in own.iter().zip(params) {
        if !eq.same(p.type_ann.as_deref()?, want) {
            return None;
        }
    }
    let r = r.map(str::trim).filter(|s| !s.is_empty()).unwrap_or("void");
    if !(ret == "void" || r == "any" || eq.same(r, ret)) {
        return None;
    }
    let key = cm.rsplit("__").next().unwrap_or_default();
    if ast.method_owners.contains_key(key) {
        Some(Body::MemberCall(own.len()))
    } else {
        Some(Body::Static(cm.to_string(), own.len()))
    }
}
