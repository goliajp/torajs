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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::ast::{Ast, Param, Stmt};
use class_row::{Fit, class_row};

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

/// One row's faces: (face, member index, thunk body).
pub(super) type Row = Vec<(String, usize, Body)>;

pub(super) struct IfacePlan {
    pub iface: String,
    pub members: Vec<Member>,
    /// (face, member index, literal thunk body)
    pub lit_faces: Row,
    /// class → (face, member index, body)
    pub classes: BTreeMap<String, Row>,
    pub with_method: bool,
    /// the classes whose instances read right at the interface's own
    /// field offsets
    pub plain: BTreeSet<String>,
    /// the classes whose rows are loose (`class_row`)
    pub loose: BTreeSet<String>,
    /// class → its data members of another type stored otherwise
    /// than the interface's (`class_row::Fit::Retyped`)
    pub retyped: BTreeMap<String, Vec<String>>,
}

/// A top-level function's type parameters, parameters and return
/// annotation.
type FnShape<'a> = (&'a [String], &'a [Param], Option<&'a str>);

struct Decls<'a> {
    fns: HashMap<&'a str, FnShape<'a>>,
    class_fields: HashMap<&'a str, &'a [(crate::ast::PropKey, String)]>,
}

pub(super) fn plan(ast: &Ast) -> Vec<IfacePlan> {
    // a loose row asks the assignability lattice, which consults the
    // witness table: the one a previous compile left must not answer
    super::admit::install(ast);
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
        let mut loose = BTreeMap::new();
        let mut retyped = BTreeMap::new();
        for &c in &classes {
            // a desugar-made class (generator / async state machines)
            // has no source spelling a program could implement with
            if c.starts_with("__")
                || crate::ast::desugar_classes_builtin_heritage::exotic_root_parent(ast, c)
                    .is_some()
            {
                continue;
            }
            if let Some((row, fit, keys)) = class_row(ast, &decls, &eq, c, &members) {
                rows.insert(c.to_string(), row);
                if fit != Fit::Exact {
                    loose.insert(c.to_string(), fit);
                }
                if !keys.is_empty() {
                    retyped.insert(c.to_string(), keys);
                }
            }
        }
        drop_unfit_lines(ast, &mut rows);
        loose.retain(|c, _| rows.contains_key(c));
        retyped.retain(|c, _| rows.contains_key(c));
        if rows.is_empty() {
            continue;
        }
        let with_method = members
            .iter()
            .any(|m| matches!(m.kind, MemberKind::Method { .. }));
        let plain = plain_classes(&decls, &classes, &members, &rows, &loose);
        let lit_faces = lit_row(&members);
        out.push(IfacePlan {
            iface: iface.to_string(),
            members,
            lit_faces,
            classes: rows,
            with_method,
            plain,
            loose: loose.into_keys().collect(),
            retyped,
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
            .all
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

/// A row is filled per class, but a subclass instance stands wherever
/// its ancestor does: a class with a descendant that has no row of its
/// own is no witness either.
fn drop_unfit_lines(ast: &Ast, rows: &mut BTreeMap<String, Row>) {
    let unfit: Vec<&String> = ast
        .class_parents
        .keys()
        .filter(|c| !rows.contains_key(c.as_str()))
        .collect();
    for c in unfit {
        let mut cur = ast.class_parents.get(c).cloned().flatten();
        let mut hops = ast.class_parents.len();
        while let Some(p) = cur {
            rows.remove(&p);
            let Some(h) = hops.checked_sub(1) else { break };
            hops = h;
            cur = ast.class_parents.get(&p).cloned().flatten();
        }
    }
}

/// The classes that lay the interface's members out first and in
/// order, each stored as the interface stores it, and whose row (if
/// any) reads none of them dynamically: their instances read right at
/// the interface's own field offsets.
fn plain_classes(
    d: &Decls<'_>,
    classes: &[&str],
    members: &[Member],
    rows: &BTreeMap<String, Row>,
    loose: &BTreeMap<String, Fit>,
) -> BTreeSet<String> {
    let prefix = |c: &str| {
        let fields = d.class_fields[c];
        members
            .iter()
            .enumerate()
            .all(|(i, m)| fields.get(i).is_some_and(|(n, _)| *n == *m.key))
    };
    // a member the class lacks is read dynamically (`DynGet`); a loose
    // one is only written that way, into a field stored alike
    let static_row = |c: &str| {
        loose.get(c) != Some(&Fit::Retyped)
            && rows
                .get(c)
                .is_none_or(|row| !row.iter().any(|(_, _, b)| matches!(b, Body::DynGet)))
    };
    classes
        .iter()
        .filter(|c| prefix(c) && static_row(c))
        .map(|c| c.to_string())
        .collect()
}

fn lit_row(members: &[Member]) -> Row {
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
