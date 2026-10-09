//! The dictionary annotation `__dict(K|V)` — an object type with an
//! index signature (TS §3.9.4), from `{ [k: K]: V }`, an interface
//! declaring one, or `Record<K, V>` — and the diagnostics on the sites
//! that spell one.
//!
//! The type keeps only the value type. What TS asks of the rest is
//! diagnosed per site ([`check_dict_sites`]): the key must be a
//! property-key type (TS1268; TS2344 for `Record`'s constraint), and a
//! named member the signature's key covers must fit the value type
//! (TS2411). A key that is a type parameter (`Record<K, V>` inside
//! `function f<K extends string>`) is not judged.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, DictSite};
use crate::check::{Checker, DiagPush, GenericAliasMap, Type};
use crate::check_assignable::is_assignable_to;

use super::markers::split_top_pipe;
use super::{resolve_type_ann_decl, resolve_type_ann_inner};

/// `inner` is the text between `__dict(` and its closing `)`. `None`
/// when the key or the value does not resolve.
pub(super) fn resolve_dict(
    inner: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut HashSet<String>,
) -> Option<Type> {
    let parts = split_top_pipe(inner);
    let [key, value] = parts.as_slice() else {
        return None;
    };
    let mut resolve =
        |ann: &str| resolve_type_ann_inner(ann, aliases, type_params, generic_aliases, in_flight);
    let key = resolve(key)?;
    Some(Type::Dict(Box::new(key), Box::new(resolve(value)?)))
}

/// The diagnostic for an annotation that does not resolve.
pub(crate) fn unknown_type(ann: &str) -> String {
    format!("unknown type `{}`", ann_text(ann))
}

/// `ann` as TS spells it where it holds a dictionary: `__dict(K|V)`
/// reads `{ [key: K]: V }`.
pub(crate) fn ann_text(ann: &str) -> String {
    let Some(at) = ann.find("__dict(") else {
        return ann.to_string();
    };
    let body = &ann[at + "__dict(".len()..];
    let mut depth = 0usize;
    let close = body
        .char_indices()
        .find(|&(_, c)| match c {
            '(' => {
                depth += 1;
                false
            }
            ')' if depth == 0 => true,
            ')' => {
                depth -= 1;
                false
            }
            _ => false,
        })
        .map_or(body.len(), |(i, _)| i);
    let dict = match split_top_pipe(&body[..close]).as_slice() {
        [k, v] => format!("{{ [key: {}]: {} }}", ann_text(k), ann_text(v)),
        _ => format!("__dict({})", &body[..close]),
    };
    let rest = body.get(close + 1..).unwrap_or("");
    format!("{}{dict}{}", &ann[..at], ann_text(rest))
}

/// Diagnose every recorded dictionary site. A part that does not
/// resolve at the top level (a type parameter, an unknown name) is
/// left to the annotation's own resolution.
pub(crate) fn check_dict_sites(c: &mut Checker, ast: &Ast) {
    let mut seen = HashSet::new();
    for site in &ast.dict_sites {
        if seen.insert(site.key_at)
            && let Some(msg) = site_error(c, site)
        {
            c.errors.push_err(msg);
        }
    }
}

fn site_error(c: &Checker, site: &DictSite) -> Option<String> {
    let resolve = |ann: &str| resolve_type_ann_decl(ann, &c.aliases, &[], &c.generic_alias_decls);
    let key = resolve(&site.key)?;
    if !is_key_type(&key) {
        let (k, at) = (&site.key_src, site.key_at);
        return Some(if site.record {
            format!(
                "type `{k}` does not satisfy the constraint `string | number | symbol` of `Record` (TS2344) at {at}"
            )
        } else {
            format!(
                "an index signature parameter type must be `string`, `number`, `symbol`, or a template literal type, not `{k}` (TS1268) at {at}"
            )
        });
    }
    let value = resolve(&site.value)?;
    for (name, ann, src, at) in &site.members {
        let fits = |ty: &Type| {
            let ty = match ty {
                // an optional member may be absent, which fits any value type
                Type::Nullable(t) => t,
                t => t,
            };
            is_assignable_to(&value, ty)
        };
        let (k, v) = (&site.key_src, &site.value_src);
        if name == crate::ast::interface_extends::EXTENDS_FIELD {
            let Some(Type::Struct(fields)) = resolve(ann) else {
                continue;
            };
            if let Some((n, _)) = fields
                .iter()
                .find(|(n, t)| key.dict_key_covers(&n.lossy()) && !fits(t))
            {
                let n = n.lossy();
                return Some(format!(
                    "property `{n}` inherited from `{src}` is not assignable to the `{k}` index type `{v}` (TS2411) at {at}"
                ));
            }
        } else if key.dict_key_covers(name)
            && let Some(ty) = resolve(ann)
            && !fits(&ty)
        {
            return Some(format!(
                "property `{name}` of type `{src}` is not assignable to the `{k}` index type `{v}` (TS2411) at {at}"
            ));
        }
    }
    None
}

fn is_key_type(t: &Type) -> bool {
    match t {
        Type::String | Type::Number | Type::Symbol => true,
        Type::Union(ms) => ms.iter().all(is_key_type),
        Type::Literal(l) => is_key_type(&l.base()),
        _ => false,
    }
}

impl Type {
    /// Whether a property named `name` falls under an index signature
    /// keyed by this type: a `string` key covers every name, a
    /// `number` key only the numeric-like names (TS §3.9.4: a
    /// canonical numeric string), a `symbol` key no spelled name. A
    /// type parameter is taken as covering every name.
    pub(crate) fn dict_key_covers(&self, name: &str) -> bool {
        match self {
            Type::String | Type::TypeVar(_) => true,
            Type::Number => is_numeric_name(name),
            Type::Union(ms) => ms.iter().any(|m| m.dict_key_covers(name)),
            // a literal key names exactly one property
            Type::Literal(crate::check::Lit::Str(s)) => s.as_str() == Some(name),
            Type::Literal(crate::check::Lit::Num(n)) => {
                crate::ast::number_prop_key(*n).as_str() == Some(name)
            }
            _ => false,
        }
    }
}

/// ES §7.1.21 CanonicalNumericIndexString: the name round-trips
/// through ToNumber and Number::toString.
fn is_numeric_name(name: &str) -> bool {
    name.parse::<f64>()
        .is_ok_and(|n| crate::ast::number_prop_key(n).as_str() == Some(name))
}
