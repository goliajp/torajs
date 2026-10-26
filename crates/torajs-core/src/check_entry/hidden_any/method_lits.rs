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

use std::collections::{HashMap, HashSet};

use super::inlobj_fields;
use super::lit_sources::Sources;
use super::method_rets::{Family, returning_any};
use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt};
use crate::check::Type;

/// A declaration's members `(name, annotation)` in declaration order,
/// and the names of those whose return is retyped. The order is part of
/// the shape: a literal whose members come in another order checks as
/// a declaration of its own (`IfaceSlots::iface_of` compares the types
/// member by member).
type Shape = (Vec<(String, String)>, HashSet<String>);

/// Respell every object type literal of the shape of an interface with
/// a member in `fam.members`, before the interface itself is retyped.
pub(super) fn retype(r: &mut Ast, fam: &Family) {
    let shapes = shapes(r, fam);
    if shapes.is_empty() {
        return;
    }
    inlobj_fields::respell(r, &|ms| {
        let shape: Vec<(String, String)> = (ms.iter())
            .map(|(k, a)| (k.trim().to_string(), a.trim().to_string()))
            .collect();
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
        if let Some(shape) = shape.filter(|_| !keys.is_empty()) {
            out.push((shape, keys));
        }
    }
    out
}

/// The functions that object literals of the type of an interface in
/// `fam.members` hold in a retyped member ([`Sources`]): the function is
/// the member's slot, called through the interface's signature, so it
/// returns its value as it is with the interface. `types` is a round's;
/// a literal's type is the interface's before the retype, or after it
/// once a previous round retyped its functions.
pub(super) fn closures(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    sources: &Sources,
    fam: &Family,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for i in ast.iface_slots.types.keys() {
        let keys: HashSet<&PropKey> = (fam.members.iter())
            .filter(|(n, _)| n == i)
            .map(|(_, k)| k)
            .collect();
        if keys.is_empty() {
            continue;
        }
        for (v, _) in member_values(ast, types, i, &keys).filter(|(_, sure)| *sure) {
            out.extend(sources.of_value(ast, v).into_iter().flatten());
        }
    }
    out
}

/// Whether an object literal that may be of the type of interface
/// `iface` holds in its member `key` a function value [`Sources`]
/// cannot follow to its declaration (a method read off an object, a
/// call's result), or one whose literal may be of another interface's
/// type too: its return cannot be retyped with the member's, and it
/// would hand its typed answer to a caller reading the member's as
/// `any`.
pub(super) fn opaque(
    ast: &Ast,
    types: &HashMap<ExprId, Type>,
    sources: &Sources,
    iface: &str,
    key: &PropKey,
) -> bool {
    let keys = HashSet::from([key]);
    member_values(ast, types, iface, &keys).any(|(v, sure)| {
        types.get(&v) != Some(&Type::Any) && (!sure || sources.of_value(ast, v).is_none())
    })
}

/// The values the object literals of the type of interface `iface`, or
/// of it with the members `keys` returning `any`, hold in a member of
/// `keys`, each with whether its literal is of that type for sure: one
/// whose members only fit the interface's ([`Fit::Loose`]) may be of
/// another interface that fits it as well.
fn member_values<'a>(
    ast: &'a Ast,
    types: &'a HashMap<ExprId, Type>,
    iface: &'a str,
    keys: &'a HashSet<&PropKey>,
) -> impl Iterator<Item = (ExprId, bool)> + 'a {
    let all = &ast.iface_slots.types;
    let want = all.get(iface);
    (ast.exprs.iter().enumerate())
        .filter_map(move |(i, e)| {
            let (Expr::ObjectLit { fields }, Some(lt), Some(want)) =
                (e, types.get(&ExprId(i as u32)), want)
            else {
                return None;
            };
            let sure = match of_iface(lt, want, keys) {
                Fit::No => return None,
                Fit::Exact => true,
                Fit::Loose => {
                    (all.iter()).all(|(n, t)| n == iface || of_iface(lt, t, keys) == Fit::No)
                }
            };
            Some((fields, sure))
        })
        .flat_map(move |(fields, sure)| {
            (fields.iter())
                .filter(|(k, _)| keys.contains(k))
                .map(move |(_, v)| (*v, sure))
        })
}

/// How an object literal typed `t` fits the interface typed `iface`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fit {
    No,
    /// `t` is `iface`, or `iface` with the members `keys` returning `any`
    Exact,
    /// so, but for members the literal spells with a type of its own
    /// that the interface's holds as it is ([`fits`])
    Loose,
}

fn of_iface(t: &Type, iface: &Type, keys: &HashSet<&PropKey>) -> Fit {
    let (Type::Struct(a), Type::Struct(b)) = (t, iface) else {
        return Fit::No;
    };
    if a.len() != b.len() {
        return Fit::No;
    }
    let mut fit = Fit::Exact;
    for ((ka, ta), (kb, tb)) in a.iter().zip(b.iter()) {
        if ka != kb {
            return Fit::No;
        }
        if ta == tb || keys.contains(ka) && returns_any_of(ta, tb) {
            continue;
        }
        if !fits(ta, tb) {
            return Fit::No;
        }
        fit = Fit::Loose;
    }
    fit
}

/// Whether a member of an object literal typed `lit` holds a value of
/// the declared type `decl` as it is: `any`, the type itself, or a
/// literal of a dictionary's values in each of its members
/// (`{ a: { m: .. } }` for `Record<string, { m(): number }>`), at any
/// depth.
fn fits(lit: &Type, decl: &Type) -> bool {
    lit == decl
        || match (lit, decl) {
            (_, Type::Any) => true,
            (Type::Struct(a), Type::Dict(_, v)) => a.iter().all(|(_, f)| fits(f, v)),
            (Type::Struct(a), Type::Struct(b)) => {
                a.len() == b.len()
                    && (a.iter().zip(b.iter())).all(|((ka, ta), (kb, tb))| ka == kb && fits(ta, tb))
            }
            (Type::Array(a), Type::Array(b)) | (Type::Nullable(a), Type::Nullable(b)) => fits(a, b),
            _ => false,
        }
}

/// Whether `ta` is the function type `tb` returning `any`.
fn returns_any_of(ta: &Type, tb: &Type) -> bool {
    match (ta, tb) {
        (Type::Function(pa, ra), Type::Function(pb, _)) => pa == pb && **ra == Type::Any,
        (Type::Nullable(a), Type::Nullable(b)) => returns_any_of(a, b),
        _ => false,
    }
}
