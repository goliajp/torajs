//! Which declared fields a program can turn into accessors.
//!
//! Only a [[DefineOwnProperty]] with an accessor descriptor does that
//! to an own data property (§10.1.6.3): `defineProperty` /
//! `defineProperties` (on `Object` or `Reflect`) and the Annex B
//! `__defineGetter__` / `__defineSetter__`. A call spelled directly
//! with a string-literal key names the one property it can reach; any
//! other spelling — a computed key, a spread argument, a descriptor
//! map that is not a literal, the definer held as a value or reached
//! by a string, any method picked by a computed key (`o[k](…)`),
//! `Object` / `Reflect` / `globalThis` held as a value or read as a
//! property — may reach any name. Like the other censuses it runs over the user's code before
//! injection, over every expression in the arena, reachable or not.

use std::collections::HashSet;

use super::super::{Ast, Expr, ExprId};

/// The answer, read by every typed field read and its throw analysis.
#[derive(Debug, Clone, Default)]
pub enum AccessorFieldNames {
    /// No definer is named: every typed field read is a plain load.
    #[default]
    None,
    /// Only these names can become accessors.
    Names(HashSet<String>),
    /// Any name can.
    All,
}

impl AccessorFieldNames {
    /// Whether a declared field called `name` can become an accessor.
    pub fn may(&self, name: &str) -> bool {
        match self {
            AccessorFieldNames::None => false,
            AccessorFieldNames::Names(n) => n.contains(name),
            AccessorFieldNames::All => true,
        }
    }

    /// Whether some declared field can.
    pub fn reaches_any(&self) -> bool {
        !matches!(self, AccessorFieldNames::None)
    }
}

const DEFINERS: &[&str] = &[
    "defineProperty",
    "defineProperties",
    "__defineGetter__",
    "__defineSetter__",
];

pub(in crate::ast) fn census(ast: &mut Ast) {
    ast.accessor_field_names = names(ast);
}

fn names(ast: &Ast) -> AccessorFieldNames {
    let mut names = HashSet::new();
    let mut callees = HashSet::new();
    let mut definer_bases = HashSet::new();
    for e in &ast.exprs {
        let (callee, args) = match e {
            Expr::Call { callee, args } | Expr::OptCall { callee, args } => (*callee, args),
            _ => continue,
        };
        // a method picked by a computed key can be any definer
        if computed_key(ast, callee) {
            return AccessorFieldNames::All;
        }
        let Expr::Member { obj, name } = ast.get_expr(callee) else {
            continue;
        };
        if !DEFINERS.contains(&name.as_str()) {
            continue;
        }
        // a spread argument moves every later argument to an unknown
        // position, the key among them
        if args
            .iter()
            .any(|a| matches!(ast.get_expr(*a), Expr::Spread { .. }))
        {
            return AccessorFieldNames::All;
        }
        let Some(keys) = literal_keys(ast, name, args) else {
            return AccessorFieldNames::All;
        };
        names.extend(keys);
        callees.insert(callee);
        definer_bases.insert(*obj);
    }
    let named_bases = super::named_bases(ast);
    let elsewhere = ast.exprs.iter().enumerate().any(|(i, e)| {
        let id = ExprId(i as u32);
        match e {
            Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
                (DEFINERS.contains(&name.as_str()) && !callees.contains(&id))
                    // `globalThis.Object` read for anything but a
                    // literal definer call
                    || (matches!(name.as_str(), "Object" | "Reflect")
                        && !definer_bases.contains(&id))
            }
            Expr::String(s) => DEFINERS.iter().any(|k| s == *k),
            Expr::Ident(n) => {
                matches!(n.as_str(), "Object" | "Reflect" | "globalThis")
                    && !named_bases.contains(&id)
            }
            _ => false,
        }
    });
    if elsewhere {
        AccessorFieldNames::All
    } else if names.is_empty() {
        AccessorFieldNames::None
    } else {
        AccessorFieldNames::Names(names)
    }
}

/// Whether `callee` picks its method by a key that is not a string
/// literal (`o[k](…)`, `o?.[k](…)`).
fn computed_key(ast: &Ast, callee: ExprId) -> bool {
    match ast.get_expr(callee) {
        Expr::Index { index, .. } | Expr::OptIndex { index, .. } => {
            !matches!(ast.get_expr(*index), Expr::String(_))
        }
        _ => false,
    }
}

/// The keys a definer call `name(args…)` spelled as literals can
/// reach; `None` when they are not all literals.
fn literal_keys(ast: &Ast, name: &str, args: &[ExprId]) -> Option<Vec<String>> {
    let literal = |k: Option<&ExprId>| match k.map(|k| ast.get_expr(*k)) {
        Some(Expr::String(s)) => s.as_str().map(|s| vec![s.to_string()]),
        _ => None,
    };
    match name {
        "defineProperty" => literal(args.get(1)),
        "__defineGetter__" | "__defineSetter__" => literal(args.first()),
        // a `__`-prefixed key is the parser's spread / computed-key
        // sentinel, never a plain name
        _ => match args.get(1).map(|m| ast.get_expr(*m)) {
            Some(Expr::ObjectLit { fields }) => (fields.iter())
                .map(|(k, _)| k.as_wtf8().as_str().filter(|k| !k.starts_with("__")))
                .map(|k| k.map(str::to_string))
                .collect(),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn census_of(src: &str) -> AccessorFieldNames {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        names(&crate::parser::parse(src, &tokens).expect("parse"))
    }

    fn is_all(n: AccessorFieldNames) -> bool {
        matches!(n, AccessorFieldNames::All)
    }

    #[test]
    fn literal_definers_name_their_keys() {
        let n = census_of(
            r#"Object.defineProperty(c, "v", d);
            Reflect.defineProperties(c, { w: d });
            c.__defineGetter__("x", f);
            globalThis.Object.defineProperty(c, "y", d);"#,
        );
        let AccessorFieldNames::Names(n) = n else {
            panic!("expected names, got {n:?}")
        };
        let mut n: Vec<_> = n.into_iter().collect();
        n.sort();
        assert_eq!(n, ["v", "w", "x", "y"]);
        assert!(matches!(
            census_of("Object.keys(c);"),
            AccessorFieldNames::None
        ));
    }

    #[test]
    fn a_spread_argument_reaches_every_name() {
        assert!(is_all(census_of(r#"Object.defineProperty(...a, "v", d);"#)));
        assert!(is_all(census_of(r#"Object.defineProperty(c, ...a);"#)));
    }

    #[test]
    fn a_method_picked_by_a_computed_key_reaches_every_name() {
        assert!(is_all(census_of(
            r#"(globalThis.Object as any)[k](c, "v", d);"#
        )));
        assert!(is_all(census_of(
            r#"({} as any).constructor[k](c, "v", d);"#
        )));
        assert!(is_all(census_of("o?.[k](c);")));
    }

    #[test]
    fn object_read_as_a_property_reaches_every_name() {
        assert!(is_all(census_of("const O = globalThis.Object;")));
        assert!(is_all(census_of("f(window.Reflect);")));
    }
}
