//! One class's row for an interface: how each face reaches the
//! class's own member, read off its field list and its `__cm_` bodies.

use super::super::{get_face, has_face, set_face};
use super::{Body, Decls, Member, MemberKind};
use crate::ast::Ast;

/// How a class's declared members match the interface's.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Fit {
    /// every member with the interface's own type
    Exact,
    /// a data member with another type that fits the interface's and
    /// is stored alike: an object, string or array under that type or
    /// nullish (`n: N` under `n: N | null`). Whether the class itself
    /// fits is the checker's structural rules' to say, as for a class
    /// with no row at all. Its get face reads the field; its set face
    /// stores through the instance's dynamic properties, which put a
    /// nullish value into the pointer slot itself.
    Loose,
    /// a loose member stored otherwise than the interface's (a
    /// `number` under `number | string`), so a read at the interface's
    /// own offsets would misread it. Its set face could not put a
    /// value of the interface's type into the field, so a field the
    /// program writes through some receiver other than `this` is held
    /// as `any` instead (`super::super::widen`), and both faces then
    /// reach it as a field.
    Retyped,
}

/// The class's thunk bodies for every face of the interface, how it
/// fits and its retyped data members, or `None` when some required
/// member is missing or does not match.
pub(super) fn class_row(
    ast: &Ast,
    d: &Decls<'_>,
    eq: &crate::ast::vtable_slot::ann_eq::AnnEq<'_>,
    c: &str,
    members: &[Member],
) -> Option<(Vec<(String, usize, Body)>, Fit, Vec<String>)> {
    let fields = d.class_fields[c];
    let field_ann = |k: &str| fields.iter().find(|(n, _)| *n == *k).map(|(_, a)| a);
    let mut row = Vec::new();
    let mut fit = Fit::Exact;
    let mut retyped = Vec::new();
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
                    Some(a) if a.trim() == "any" => {
                        fit = fit.max(Fit::Retyped);
                        (Body::FieldGet, Body::FieldSet)
                    }
                    Some(a) if eq.fits(&m.ann, a) => {
                        if eq.nullable_of(&m.ann, a) {
                            fit = fit.max(Fit::Loose);
                        } else {
                            fit = fit.max(Fit::Retyped);
                            retyped.push(m.key.clone());
                        }
                        (Body::FieldGet, Body::DynSet)
                    }
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
    Some((row, fit, retyped))
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

/// How a thunk calls the method body `cm`: directly. `None` when its
/// signature does not match the interface's.
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
    // the row is only ever reached from an instance of exactly this
    // class, and `cm` is the body that class answers with: no override
    // is left to pick
    Some(Body::Static(cm.to_string(), own.len()))
}
