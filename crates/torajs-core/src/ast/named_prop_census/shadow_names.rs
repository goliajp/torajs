//! Which declared fields a program can write a value their slot has no
//! spelling for.
//!
//! A layout slot is typed; the spec's property takes any value. A write
//! the typed store cannot make — through a receiver the checker does
//! not know as the struct (`(p as any).a = v`, an `any` binding, a
//! computed key), or a typed store whose value is an any-lane value or
//! a `null` into a number slot — runs the runtime's [[Set]], which fits
//! what the slot can hold and parks the rest in the field's sidecar
//! behind `FLAG_OBJ_EXOTIC_FIELD`. A typed read of such a field has to
//! test that bit and answer the sidecar, so this census names the
//! fields the program can shadow; every other field keeps its bare
//! load. It needs the checker's types (the receiver's, the value's),
//! so it runs after checking, over every expression in the arena.
//!
//! The written value's type narrows the reach: a `number` fits an f64
//! slot, so a write whose value the checker knows as a number
//! (`counts[w] = n`) shadows only the slots of other types, and a
//! typed read of an optional `number` member keeps its f64 load.
//!
//! A cast from the any lane into an annotated layout
//! (`const p: Opt = JSON.parse(s)`) writes too: a `null` where the
//! layout has an optional `number` member goes to the sidecar the same
//! way (`super::cast_targets`).

use std::collections::{HashMap, HashSet};

use super::accessor_names::{AccessorFieldNames, Reach};
use crate::ast::{Ast, Expr, ExprId, PropKey};
use crate::check::{GenericAliasMap, Lit, Type};

/// What a written value may be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A number: it fits an f64 slot and shadows every other.
    Number,
    /// Anything: it may shadow any slot.
    Unknown,
}

enum Hit {
    None,
    Name(String, Kind),
    All(Kind),
}

#[derive(Default)]
struct Names {
    unknown: HashSet<String>,
    number: HashSet<String>,
    all_unknown: bool,
    all_number: bool,
}

impl Names {
    fn take(&mut self, hit: Hit) {
        match hit {
            Hit::None => {}
            Hit::Name(n, Kind::Unknown) => {
                self.unknown.insert(n);
            }
            Hit::Name(n, Kind::Number) => {
                self.number.insert(n);
            }
            Hit::All(Kind::Unknown) => self.all_unknown = true,
            Hit::All(Kind::Number) => self.all_number = true,
        }
    }

    fn reach(all: bool, names: HashSet<String>) -> Reach {
        match (all, names.is_empty()) {
            (true, _) => Reach::All,
            (false, true) => Reach::None,
            (false, false) => Reach::Names(names),
        }
    }
}

impl AccessorFieldNames {
    /// Record which declared fields the program's writes can shadow.
    /// Runs after [`AccessorFieldNames::settle_key_reads`]: a program
    /// whose definer calls reach every name (a method picked by a
    /// computed key, `Reflect` held as a value) can reach `Reflect.set`
    /// the same way.
    pub fn settle_shadow_writes(
        ast: &mut Ast,
        expr_types: &HashMap<ExprId, Type>,
        aliases: &HashMap<String, Type>,
        generic_aliases: &GenericAliasMap,
    ) {
        let optional_number = crate::num_width::optional_number_fields::names(ast, expr_types);
        ast.accessor_field_names.optional_number = optional_number.clone();
        if matches!(ast.accessor_field_names.reach, Reach::All) {
            ast.accessor_field_names.shadow = Reach::All;
            return;
        }
        let twins = crate::ast::twin_body_exprs(ast);
        let mut names = Names::default();
        for (i, e) in ast.exprs.iter().enumerate() {
            let in_twin = twins.contains(&ExprId(i as u32));
            let hit = match e {
                Expr::Assign { target, value } if in_twin => {
                    twin_write(ast, expr_types, &optional_number, *target, *value)
                }
                _ if in_twin => Hit::None,
                Expr::Assign { target, value } => write(ast, expr_types, *target, Some(*value)),
                Expr::PostIncr { target, .. } => write(ast, expr_types, *target, None),
                Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                    call(ast, expr_types, *callee, args)
                }
                Expr::ObjectLit { fields } => literal(expr_types, &optional_number, fields),
                _ => Hit::None,
            };
            names.take(hit);
        }
        let casts = super::cast_targets::names(
            ast,
            expr_types,
            (aliases, generic_aliases),
            &optional_number,
        );
        names.unknown.extend(casts.number_names);
        ast.accessor_field_names.cast_leaves_members_out = casts.optional;
        ast.accessor_field_names.shadow = Names::reach(names.all_unknown, names.unknown);
        ast.accessor_field_names.shadow_number = Names::reach(names.all_number, names.number);
    }
}

/// A write to `target`, with `value` its rhs (none for `++` / `--`,
/// whose result is a number unless the old value was a BigInt the
/// sidecar already holds).
fn write(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    target: ExprId,
    value: Option<ExprId>,
) -> Hit {
    let kind = value.map_or(Kind::Number, |v| kind_of(expr_types.get(&v)));
    match ast.get_expr(target) {
        Expr::Member { obj, name } => {
            if !struct_like(expr_types.get(obj)) {
                return Hit::Name(name.clone(), kind);
            }
            // a typed store: the slot takes a typed value; a number slot
            // handed an any-lane value or a `null` joins the any lane
            let unspelled = value.is_some_and(|v| may_hold_unspelled(expr_types.get(&v)));
            match unspelled && number_field(ast, expr_types, *obj, name) {
                true => Hit::Name(name.clone(), Kind::Unknown),
                false => Hit::None,
            }
        }
        Expr::Index { obj, index } => {
            if !property_receiver(expr_types.get(obj)) {
                return Hit::None;
            }
            match ast.get_expr(*index) {
                Expr::String(s) => match s.as_str() {
                    Some(s) => Hit::Name(s.to_string(), kind),
                    None => Hit::All(kind),
                },
                Expr::Number(n) => match crate::ast::number_prop_key(*n).as_wtf8().as_str() {
                    Some(s) => Hit::Name(s.to_string(), kind),
                    None => Hit::All(kind),
                },
                _ if matches!(expr_types.get(obj), Some(Type::Any))
                    && !key_may_name_field(expr_types.get(index)) =>
                {
                    Hit::None
                }
                _ => Hit::All(kind),
            }
        }
        _ => Hit::None,
    }
}

/// A write inside a receiver-polymorphic twin (`ast::twin_body_exprs`),
/// the compiler's clone of a method body with every parameter retyped
/// `any`. Its writes are the original's, already counted — except that
/// `this.m = v` with a parameter `v` now carries any value into an
/// optional `number` member's f64 slot, which the original's typed
/// store never did.
fn twin_write(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    optional_number: &HashSet<PropKey>,
    target: ExprId,
    value: ExprId,
) -> Hit {
    let Expr::Member { obj, name } = ast.get_expr(target) else {
        return Hit::None;
    };
    let this = matches!(ast.get_expr(*obj), Expr::This);
    let unspelled = may_hold_unspelled(expr_types.get(&value));
    match this && unspelled && optional_number.contains(&PropKey::from(name.as_str())) {
        true => Hit::Name(name.clone(), Kind::Unknown),
        false => Hit::None,
    }
}

/// A bulk or reflective write: `Reflect.set` names its key when the
/// key is a literal; `Object.assign` reaches every own field of its
/// sources, each with its own type when the source is a struct, and
/// every name otherwise.
fn call(ast: &Ast, expr_types: &HashMap<ExprId, Type>, callee: ExprId, args: &[ExprId]) -> Hit {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return Hit::None;
    };
    let base = match ast.get_expr(*obj) {
        Expr::Ident(n) => n.as_str(),
        _ => return Hit::None,
    };
    match (base, name.as_str()) {
        ("Reflect", "set") => {
            let kind = args
                .get(2)
                .map_or(Kind::Unknown, |v| kind_of(expr_types.get(v)));
            match args.get(1).map(|k| ast.get_expr(*k)) {
                Some(Expr::String(s)) => match s.as_str() {
                    Some(s) => Hit::Name(s.to_string(), kind),
                    None => Hit::All(kind),
                },
                _ => Hit::All(kind),
            }
        }
        ("Object", "assign") => assign_sources(ast, expr_types, args.get(1..).unwrap_or(&[])),
        _ => Hit::None,
    }
}

/// The fields `Object.assign` copies out of `sources`: one hit covers
/// them all, so a source whose fields are not all known settles for
/// every name.
fn assign_sources(ast: &Ast, expr_types: &HashMap<ExprId, Type>, sources: &[ExprId]) -> Hit {
    let mut number = true;
    let mut names: Vec<String> = Vec::new();
    for s in sources {
        if matches!(ast.get_expr(*s), Expr::Spread { .. }) {
            return Hit::All(Kind::Unknown);
        }
        let Some(Type::Struct(fs)) = expr_types.get(s) else {
            return Hit::All(Kind::Unknown);
        };
        for (k, t) in fs.iter() {
            let Some(k) = k.as_wtf8().as_str() else {
                return Hit::All(Kind::Unknown);
            };
            number &= kind_of(Some(t)) == Kind::Number;
            names.push(k.to_string());
        }
    }
    // the hit names one field; a struct source with several of mixed
    // kinds takes the wider one for them all
    match names.len() {
        0 => Hit::None,
        1 => Hit::Name(names.remove(0), kind(number)),
        _ => Hit::All(kind(number)),
    }
}

fn kind(number: bool) -> Kind {
    match number {
        true => Kind::Number,
        false => Kind::Unknown,
    }
}

/// A literal whose member named like an optional `number` member takes
/// a value an f64 slot cannot spell: stored as that type, the lowering
/// stores the value through the any lane once the cell exists.
fn literal(
    expr_types: &HashMap<ExprId, Type>,
    optional_number: &HashSet<PropKey>,
    fields: &[(PropKey, ExprId)],
) -> Hit {
    for (k, v) in fields {
        if optional_number.contains(k)
            && may_hold_unspelled(expr_types.get(v))
            && let Some(k) = k.as_wtf8().as_str()
        {
            return Hit::Name(k.to_string(), Kind::Unknown);
        }
    }
    Hit::None
}

/// What a value of this type may be once written.
fn kind_of(t: Option<&Type>) -> Kind {
    match t {
        Some(Type::Number | Type::Literal(Lit::Num(_))) => Kind::Number,
        _ => Kind::Unknown,
    }
}

/// A receiver the lowering stores into through the layout.
fn struct_like(t: Option<&Type>) -> bool {
    match t {
        Some(Type::Struct(_) | Type::ClassRef(_)) => true,
        Some(Type::Nullable(t)) => struct_like(Some(t)),
        _ => false,
    }
}

/// A keyed write that may reach a declared field: on an array, a
/// string, a collection or a buffer the key picks an element, and a
/// dictionary (`Record<K, V>`) never holds a struct cell — the checker
/// admits no struct type into one (`check_assignable`).
fn property_receiver(t: Option<&Type>) -> bool {
    match t {
        None | Some(Type::Any | Type::Struct(_) | Type::ClassRef(_) | Type::Object(_)) => true,
        Some(Type::TypeVar(_)) => true,
        Some(Type::Nullable(t)) => property_receiver(Some(t)),
        Some(Type::Union(ts)) => ts.iter().any(|t| property_receiver(Some(t))),
        _ => false,
    }
}

/// Whether a key of this type can spell a declared field's name: a
/// number picks an element of an `any` receiver, not a field.
fn key_may_name_field(t: Option<&Type>) -> bool {
    !matches!(t, Some(Type::Number | Type::Literal(Lit::Num(_))))
}

/// A value the lowering may hand a number slot as an any-lane value or
/// a `null`: the any lane itself, `null`, or a type the checker spells
/// `T | null`, whose storage may be a box.
fn may_hold_unspelled(t: Option<&Type>) -> bool {
    matches!(
        t,
        None | Some(Type::Any | Type::Null | Type::Nullable(_) | Type::Union(_))
    )
}

/// Whether the field `name` of the struct-typed receiver `obj` is a
/// number slot: `number`, or an optional `number`. An unknown field
/// counts.
fn number_field(ast: &Ast, expr_types: &HashMap<ExprId, Type>, obj: ExprId, name: &str) -> bool {
    let mut t = expr_types.get(&obj);
    while let Some(Type::Nullable(inner)) = t {
        t = Some(inner);
    }
    match t {
        Some(Type::Struct(fs)) => fs.iter().find(|(k, _)| *k == name).is_none_or(|(_, t)| {
            matches!(t, Type::Number) || matches!(t, Type::Nullable(n) if **n == Type::Number)
        }),
        _ => crate::ast_throw_info::class_field_ann(ast, expr_types, obj, name)
            .is_none_or(|a| a == "number" || a == crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settled(src: &str, types: impl Fn(&Ast) -> HashMap<ExprId, Type>) -> AccessorFieldNames {
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let mut ast = crate::parser::parse(src, &tokens).expect("parse");
        let types = types(&ast);
        AccessorFieldNames::settle_shadow_writes(
            &mut ast,
            &types,
            &HashMap::new(),
            &HashMap::new(),
        );
        ast.accessor_field_names
    }

    /// The type of every `Ident(name)` in the arena, and `number` for
    /// every numeric literal.
    fn ident_types(ast: &Ast, name: &str, t: Type) -> HashMap<ExprId, Type> {
        (ast.exprs.iter().enumerate())
            .filter_map(|(i, e)| match e {
                Expr::Ident(n) if n == name => Some((ExprId(i as u32), t.clone())),
                Expr::Number(_) => Some((ExprId(i as u32), Type::Number)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_write_through_an_any_receiver_names_its_field() {
        let n = settled("x.a = null; x.b++;", |ast| ident_types(ast, "x", Type::Any));
        assert!(n.may_shadow("a", true) && n.may_shadow("a", false));
        // a `++` writes a number: it fits an f64 slot
        assert!(!n.may_shadow("b", true) && n.may_shadow("b", false));
        assert!(!n.may_shadow("c", false));
    }

    #[test]
    fn a_typed_store_of_a_number_keeps_the_bare_load() {
        let fs = crate::check::Type::struct_of(vec![
            (PropKey::from("a"), Type::Number),
            (PropKey::from("s"), Type::String),
        ]);
        let n = settled("p.a = 1; p.s = v;", |ast| {
            let mut t = ident_types(ast, "p", fs.clone());
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(!n.may_shadow("a", true) && !n.may_shadow("s", false));
        let n = settled("p.a = v; p.a = null;", |ast| {
            let mut t = ident_types(ast, "p", fs.clone());
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(n.may_shadow("a", true));
    }

    #[test]
    fn a_computed_key_reaches_every_name_unless_it_indexes() {
        let n = settled("p[k] = v;", |ast| {
            let mut t = ident_types(ast, "p", Type::Any);
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(n.may_shadow("anything", true));
        let n = settled("xs[i] = 1;", |ast| {
            let mut t = ident_types(ast, "xs", Type::Array(Box::new(Type::Number)));
            t.extend(ident_types(ast, "i", Type::Number));
            t
        });
        assert!(!n.may_shadow("a", false));
        let n = settled("p[0] = 1; q[\"b\"] = 2;", |ast| {
            ident_types(ast, "p", Type::Any)
        });
        assert!(n.may_shadow("0", false) && n.may_shadow("b", false));
        assert!(!n.may_shadow("b", true) && !n.may_shadow("c", false));
    }

    #[test]
    fn a_number_written_under_a_computed_key_spares_the_f64_slots() {
        let n = settled("o[w] = n;", |ast| {
            let mut t = ident_types(ast, "o", Type::Any);
            t.extend(ident_types(ast, "n", Type::Number));
            t
        });
        assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
        // a dictionary never holds a struct cell
        let n = settled("counts[w] = v;", |ast| {
            let mut t = ident_types(
                ast,
                "counts",
                Type::Dict(Box::new(Type::String), Box::new(Type::Number)),
            );
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(!n.may_shadow("a", true) && !n.may_shadow("a", false));
    }

    #[test]
    fn reflective_writes_reach_their_keys() {
        let n = settled(r#"Reflect.set(p, "a", v);"#, |ast| {
            ident_types(ast, "v", Type::Any)
        });
        assert!(n.may_shadow("a", true) && !n.may_shadow("b", true));
        assert!(settled("Reflect.set(p, k, v);", |_| HashMap::new()).may_shadow("zz", true));
        let n = settled("Reflect.set(p, k, 1);", |ast| {
            ident_types(ast, "k", Type::String)
        });
        assert!(!n.may_shadow("zz", true) && n.may_shadow("zz", false));
    }

    #[test]
    fn object_assign_reaches_the_fields_of_its_sources() {
        let src = crate::check::Type::struct_of(vec![
            (PropKey::from("a"), Type::Number),
            (PropKey::from("b"), Type::Number),
        ]);
        let n = settled("Object.assign(p, q);", |ast| {
            ident_types(ast, "q", src.clone())
        });
        assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
        let n = settled("Object.assign(p, q);", |ast| {
            ident_types(ast, "q", Type::Any)
        });
        assert!(n.may_shadow("zz", true));
        let one = crate::check::Type::struct_of(vec![(PropKey::from("s"), Type::String)]);
        let n = settled("Object.assign(p, q);", |ast| {
            ident_types(ast, "q", one.clone())
        });
        assert!(n.may_shadow("s", true) && !n.may_shadow("t", false));
    }
}
