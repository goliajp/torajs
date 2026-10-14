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
//!
//! One spelling needs the checker's types: a computed-key read
//! (`O["define" + "Property"]`) hands a definer out as a value when its
//! receiver can hold `Object` or `Reflect`, which a value of type `any`
//! can (`({} as any).constructor`). The census records those reads,
//! [`AccessorFieldNames::settle_key_reads`] answers them once the
//! types are known, and until then each one reaches every name.

use std::collections::{HashMap, HashSet};

use super::super::{Ast, Expr, ExprId};
use definer_types::{may_hold_definer, may_spell_definer};

mod definer_types;

/// The answer, read by every typed field read and its throw analysis.
#[derive(Debug, Clone, Default)]
pub struct AccessorFieldNames {
    pub(super) reach: Reach,
    /// Computed-key reads whose receiver's type is not settled yet.
    key_reads: Vec<ExprId>,
    /// What the shadow census found (`super::shadow_facts`).
    pub shadowed: super::shadow_facts::ShadowFacts,
}

#[derive(Debug, Clone, Default)]
pub(super) enum Reach {
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
        match &self.reach {
            _ if !self.key_reads.is_empty() => true,
            Reach::None => false,
            Reach::Names(n) => n.contains(name),
            Reach::All => true,
        }
    }

    /// Whether some declared field can.
    pub fn reaches_any(&self) -> bool {
        !self.key_reads.is_empty() || !matches!(self.reach, Reach::None)
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

impl AccessorFieldNames {
    /// Answer the computed-key reads the census left open: one whose
    /// receiver can hold a namespace or a function (`any` / `unknown`, a
    /// function type, a dictionary, a type parameter, or no recorded
    /// type) and whose key can be a string the program never spells (a
    /// number, a symbol or a literal type cannot) may pick a definer,
    /// and then every name is reachable.
    pub fn settle_key_reads(ast: &mut Ast, expr_types: &HashMap<ExprId, crate::check::Type>) {
        let open = ast.accessor_field_names.key_reads_open(ast, expr_types);
        ast.accessor_field_names.key_reads.clear();
        if open {
            ast.accessor_field_names.reach = Reach::All;
        }
    }

    /// Whether some computed-key read the census left open may pick a
    /// definer, judged by `expr_types`.
    pub(super) fn key_reads_open(
        &self,
        ast: &Ast,
        expr_types: &HashMap<ExprId, crate::check::Type>,
    ) -> bool {
        // a read the checker never reached (rewritten away by a pass, or
        // in a body nothing calls) is not one the program performs
        let reached = |e: &ExprId| expr_types.contains_key(e);
        self.key_reads.iter().any(|r| match ast.get_expr(*r) {
            Expr::Index { obj, index } | Expr::OptIndex { obj, index } => {
                (reached(r) || reached(obj))
                    && expr_types.get(obj).is_none_or(may_hold_definer)
                    && expr_types.get(index).is_none_or(may_spell_definer)
            }
            // rewritten in place since the census: nothing says the
            // new shape is harmless
            _ => reached(r),
        })
    }
}

fn names(ast: &Ast) -> AccessorFieldNames {
    let reach = reach(ast);
    let key_reads = match reach {
        Reach::All => Vec::new(),
        _ => (ast.exprs.iter().enumerate())
            .filter(|(_, e)| computed_read(ast, e))
            .map(|(i, _)| ExprId(i as u32))
            .collect(),
    };
    AccessorFieldNames {
        reach,
        key_reads,
        shadowed: Default::default(),
    }
}

/// A read whose key is not a literal (`o[k]`, `o?.[k]`).
fn computed_read(ast: &Ast, e: &Expr) -> bool {
    match e {
        Expr::Index { index, .. } | Expr::OptIndex { index, .. } => {
            !matches!(ast.get_expr(*index), Expr::String(_) | Expr::Number(_))
        }
        _ => false,
    }
}

fn reach(ast: &Ast) -> Reach {
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
            return Reach::All;
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
            return Reach::All;
        }
        let Some(keys) = literal_keys(ast, name, args) else {
            return Reach::All;
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
        Reach::All
    } else if names.is_empty() {
        Reach::None
    } else {
        Reach::Names(names)
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

    fn parse(src: &str) -> Ast {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        crate::parser::parse(src, &tokens).expect("parse")
    }

    fn census_of(src: &str) -> AccessorFieldNames {
        names(&parse(src))
    }

    fn is_all(n: AccessorFieldNames) -> bool {
        matches!(n.reach, Reach::All)
    }

    #[test]
    fn literal_definers_name_their_keys() {
        let n = census_of(
            r#"Object.defineProperty(c, "v", d);
            Reflect.defineProperties(c, { w: d });
            c.__defineGetter__("x", f);
            globalThis.Object.defineProperty(c, "y", d);"#,
        );
        let Reach::Names(n) = n.reach else {
            panic!("expected names, got {n:?}")
        };
        let mut n: Vec<_> = n.into_iter().collect();
        n.sort();
        assert_eq!(n, ["v", "w", "x", "y"]);
        assert!(!census_of("Object.keys(c);").reaches_any());
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

    /// `O[k]` reaches every name exactly when its receiver's type can
    /// hold a namespace and its key's type can be a definer's name; an
    /// unsettled read already counts as reaching every name, and one the
    /// checker never reached reaches none.
    #[test]
    fn a_computed_read_reaches_every_name_by_its_types() {
        use crate::check::{Lit, Type};
        let settle = |read_ty: Option<Type>, obj_ty: Option<Type>, key_ty: Option<Type>| {
            let mut ast = parse(r#"const dp = O[k]; dp(c, "v", d);"#);
            ast.accessor_field_names = names(&ast);
            assert!(ast.accessor_field_names.may("v"));
            let (read, obj, index) = (ast.exprs.iter().enumerate())
                .find_map(|e| match e {
                    (i, Expr::Index { obj, index }) => Some((ExprId(i as u32), *obj, *index)),
                    _ => None,
                })
                .expect("the computed read");
            let types: HashMap<ExprId, Type> = [(read, read_ty), (obj, obj_ty), (index, key_ty)]
                .into_iter()
                .filter_map(|(e, t)| Some((e, t?)))
                .collect();
            AccessorFieldNames::settle_key_reads(&mut ast, &types);
            ast.accessor_field_names.may("v")
        };
        let lit = |s: &str| Type::Literal(Lit::Str(s.into()));
        let any = || Some(Type::Any);
        assert!(settle(any(), any(), Some(Type::String)));
        assert!(settle(
            any(),
            Some(Type::Nullable(Box::new(Type::Any))),
            any()
        ));
        assert!(settle(any(), None, None));
        assert!(!settle(None, None, None));
        assert!(settle(any(), any(), Some(lit("defineProperty"))));
        assert!(!settle(any(), any(), Some(Type::Number)));
        assert!(!settle(any(), any(), Some(lit("x"))));
        let nums = Some(Type::Array(Box::new(Type::Number)));
        assert!(!settle(any(), nums, Some(Type::String)));
        assert!(!settle(any(), Some(Type::String), any()));
    }
}
