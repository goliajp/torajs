//! Which interfaces dispatch through witness slots, and which classes
//! fill a row for them — read off the desugared declarations only.
//!
//! A class is a witness of an interface when every required member is
//! declared on it (or inherited) with the interface's types, compared
//! by annotation. A comparison that cannot be settled this way leaves
//! the class out, and the checker then refuses that class where the
//! interface is expected: the refusal is the conservative answer, a
//! row read through the wrong signature is not.

mod class_row;

use std::collections::{BTreeMap, HashMap};

use crate::ast::{Ast, Param, Stmt};
use class_row::class_row;

pub(super) struct Member {
    pub key: String,
    /// the declaration's own annotation
    pub ann: String,
    pub kind: MemberKind,
    pub optional: bool,
}

pub(super) enum MemberKind {
    Method { params: Vec<String>, ret: String },
    Data,
}

/// How a thunk reaches its member.
pub(super) enum Body {
    /// a direct call to the named body, passing the first `n` arguments
    Static(String, usize),
    /// `__self.key(..)` through the ordinary member-call lanes
    MemberCall(usize),
    /// a literal's optional method: call it when present, else let the
    /// dynamic call raise the TypeError
    GuardedCall,
    /// a member the class does not declare (optional in the interface):
    /// the instance's own dynamic properties answer
    DynCall,
    FieldGet,
    FieldSet,
    DynGet,
    DynSet,
    /// an optional method the class declares: always present
    HasTrue,
    /// an optional method held in a field (or a literal's member)
    HasField,
    /// an optional method the class does not declare
    HasDyn,
}

pub(super) struct IfacePlan {
    pub iface: String,
    pub members: Vec<Member>,
    /// (face, member index, literal thunk body)
    pub lit_faces: Vec<(String, usize, Body)>,
    /// class → (face, member index, body)
    pub classes: BTreeMap<String, Vec<(String, usize, Body)>>,
}

/// A top-level function's type parameters, parameters and return
/// annotation.
type FnShape<'a> = (&'a [String], &'a [Param], Option<&'a str>);

struct Decls<'a> {
    fns: HashMap<&'a str, FnShape<'a>>,
    class_fields: HashMap<&'a str, &'a [(crate::ast::PropKey, String)]>,
}

pub(super) fn plan(ast: &Ast) -> Vec<IfacePlan> {
    let mut decls = Decls {
        fns: HashMap::new(),
        class_fields: HashMap::new(),
    };
    let mut ifaces: Vec<(&str, Vec<Member>)> = Vec::new();
    for s in crate::ast::toplevel_stmts_flat(ast) {
        match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                ..
            } => {
                decls
                    .fns
                    .insert(name, (type_params, params, return_type.as_deref()));
            }
            Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } if type_params.is_empty() => {
                if ast.class_parents.contains_key(name) {
                    decls.class_fields.insert(name, fields);
                } else if let Some(ms) = members_of(ast, name, fields) {
                    ifaces.push((name, ms));
                }
            }
            _ => {}
        }
    }
    let eq = crate::ast::vtable_slot::ann_eq::AnnEq::new(ast);
    let mut classes: Vec<&str> = decls.class_fields.keys().copied().collect();
    classes.sort();
    let mut out = Vec::new();
    for (iface, members) in ifaces {
        let mut rows = BTreeMap::new();
        for &c in &classes {
            // a desugar-made class (generator / async state machines)
            // has no source spelling a program could implement with
            if c.starts_with("__")
                || crate::ast::desugar_classes_builtin_heritage::exotic_root_parent(ast, c)
                    .is_some()
            {
                continue;
            }
            if let Some(row) = class_row(ast, &decls, &eq, c, &members) {
                rows.insert(c.to_string(), row);
            }
        }
        if rows.is_empty() || !needs_slots(&decls, &members, &rows) {
            continue;
        }
        let lit_faces = lit_row(&members);
        out.push(IfacePlan {
            iface: iface.to_string(),
            members,
            lit_faces,
            classes: rows,
        });
    }
    out
}

/// The members of an object type declaration, or `None` when one of
/// its keys is not a plain name (a computed or internal key has no
/// member a class could declare under it).
fn members_of(
    ast: &Ast,
    iface: &str,
    fields: &[(crate::ast::PropKey, String)],
) -> Option<Vec<Member>> {
    if fields.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (k, ann) in fields {
        let key = k.as_str()?;
        let plain = key
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '$')
            && key
                .chars()
                .all(|c| c.is_alphanumeric() || c == '$' || c == '_')
            && !key.contains("__");
        if !plain {
            return None;
        }
        let optional = ast
            .optional_members
            .contains(&(iface.to_string(), k.clone()));
        let kind = match crate::num_width::fn_type_canon(ann)
            .as_deref()
            .and_then(crate::num_width::split_fn_type)
        {
            Some((ps, r)) => MemberKind::Method {
                params: ps
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect(),
                ret: r.to_string(),
            },
            None => MemberKind::Data,
        };
        out.push(Member {
            key: key.to_string(),
            ann: ann.clone(),
            kind,
            optional,
        });
    }
    Some(out)
}

/// Whether some witness needs the slots: the interface has a method
/// (a class keeps those out of the instance), reads a member the class
/// does not declare, or the class does not lay the interface's fields
/// out first and in order.
fn needs_slots(
    d: &Decls<'_>,
    members: &[Member],
    rows: &BTreeMap<String, Vec<(String, usize, Body)>>,
) -> bool {
    if members
        .iter()
        .any(|m| matches!(m.kind, MemberKind::Method { .. }))
    {
        return true;
    }
    rows.iter().any(|(c, row)| {
        let fields = d.class_fields[c.as_str()];
        row.iter()
            .any(|(_, _, b)| matches!(b, Body::DynGet | Body::DynSet))
            || members
                .iter()
                .enumerate()
                .any(|(i, m)| fields.get(i).is_none_or(|(n, _)| *n != *m.key))
    })
}

fn lit_row(members: &[Member]) -> Vec<(String, usize, Body)> {
    let mut out = Vec::new();
    for (i, m) in members.iter().enumerate() {
        match &m.kind {
            MemberKind::Method { params, .. } => {
                let body = if m.optional {
                    out.push((super::has_face(&m.key), i, Body::HasField));
                    Body::GuardedCall
                } else {
                    Body::MemberCall(params.len())
                };
                out.push((m.key.clone(), i, body));
                out.push((super::get_face(&m.key), i, Body::FieldGet));
            }
            MemberKind::Data => {
                out.push((super::get_face(&m.key), i, Body::FieldGet));
                out.push((super::set_face(&m.key), i, Body::FieldSet));
            }
        }
    }
    out
}
