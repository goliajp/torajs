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
    /// Several names, each with the kind of its own value.
    Names(Vec<(String, Kind)>),
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
            Hit::Names(ns) => {
                for (n, k) in ns {
                    self.take(Hit::Name(n, k));
                }
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
        (aliases, generic_aliases): (&HashMap<String, Type>, &GenericAliasMap),
        mistyped: &ShadowedNames,
    ) {
        ast.accessor_field_names.mistyped = mistyped.clone();
        let optional_number = crate::num_width::optional_number_fields::names(ast, expr_types);
        ast.accessor_field_names.optional_number = optional_number.clone();
        if matches!(ast.accessor_field_names.reach, Reach::All) {
            ast.accessor_field_names.shadow = Reach::All;
            return;
        }
        let fields = &ast.accessor_field_names;
        let mut names = writes(ast, expr_types, &optional_number, &|n| fields.may(n));
        let casts = super::cast_targets::names(
            ast,
            expr_types,
            (aliases, generic_aliases),
            &optional_number,
        );
        names.unknown.extend(casts.number_names);
        let mut leaves_members_out = casts.optional;
        // a read that converts a shadowed field's value into its layout
        // is a cast too; the names it adds are optional `number`
        // members, which may name a layout field of another type
        loop {
            let reached = |k: &str| {
                names.all_unknown || names.unknown.contains(k) || ast.accessor_field_names.may(k)
            };
            let conv = super::cast_targets::converted_fields(
                ast,
                expr_types,
                (aliases, generic_aliases),
                &optional_number,
                &reached,
            );
            leaves_members_out |= conv.optional;
            if conv.number_names.is_subset(&names.unknown) {
                break;
            }
            names.unknown.extend(conv.number_names);
        }
        ast.accessor_field_names.cast_leaves_members_out = leaves_members_out;
        ast.accessor_field_names.shadow = Names::reach(names.all_unknown, names.unknown);
        ast.accessor_field_names.shadow_number = Names::reach(names.all_number, names.number);
    }
}

/// The member names the program's writes can leave holding a value
/// their slot has no spelling for, each set by the kind of the value
/// (`ShadowedNames`). A write that may reach every name (a computed key
/// on an `any` receiver) is no name of its own and is left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowedNames {
    /// Written a value of any kind.
    pub unknown: HashSet<String>,
    /// Written a number.
    pub number: HashSet<String>,
}

impl ShadowedNames {
    pub fn of(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> Self {
        let accessor = ast.accessor_field_names.may_settled(ast, expr_types);
        let names = writes(ast, expr_types, &HashSet::new(), &accessor);
        ShadowedNames {
            unknown: names.unknown,
            number: names.number,
        }
    }

    pub fn is_subset(&self, other: &ShadowedNames) -> bool {
        self.unknown.is_subset(&other.unknown) && self.number.is_subset(&other.number)
    }

    pub fn extend(&mut self, other: ShadowedNames) {
        self.unknown.extend(other.unknown);
        self.number.extend(other.number);
    }

    /// Whether a member of the checked type `t` lives in a number slot,
    /// which every number a write leaves there fits: `number`, and a
    /// number literal type (`k: 1`), whose slot is the f64 `number`'s.
    /// The lowering reads the same answer off the slot's type
    /// (`shadow_read::mistyped_field`); the two have to agree, or the
    /// checker retypes a read the lowering keeps as a bare number.
    pub fn number_member(t: &Type) -> bool {
        matches!(t, Type::Number | Type::Literal(Lit::Num(_)))
    }

    /// Whether the member `name`, whose slot holds a `number` when
    /// `number_slot`, may hold a value of another type. A member the
    /// compiler mints (`__state`, `__sent`, an awaited value's slot)
    /// is read by the code it generates, which casts what the protocol
    /// hands it; its reads keep their type and their guard.
    pub fn may_mistype(&self, name: &str, number_slot: bool) -> bool {
        !name.starts_with("__")
            && (self.unknown.contains(name) || (!number_slot && self.number.contains(name)))
    }
}

/// Every write of the program, by the names it reaches. A
/// receiver-polymorphic twin's writes count like any other once the
/// twin can run: it types its receiver, parameters and locals `any`, so
/// its `t.m = v` is an any-lane [[Set]] onto whatever cell the caller
/// handed it, and the value is a number only when `write_kind` shows it
/// is one. A twin nothing enters (`idle_twin_exprs`) writes nothing. That
/// verdict reads which members the writes leave holding values of any
/// kind, so the census repeats until that set stops growing.
fn writes(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    optional_number: &HashSet<PropKey>,
    accessor: &dyn Fn(&str) -> bool,
) -> Names {
    // built only when some write's value needs its number-ness judged
    let layouts = std::cell::OnceCell::new();
    // traced only when some write names a member through the any lane
    let origins = std::cell::OnceCell::new();
    let origins = || origins.get_or_init(|| super::receiver_origin::Origins::of(ast, expr_types));
    let dead_twins = crate::ssa_lower_pass_0_5::arr_head_census::idle_twin_exprs(ast, expr_types);
    let mut unknown: HashSet<String> = HashSet::new();
    loop {
        let facts = super::write_kind::Facts {
            ast,
            expr_types,
            layouts: &layouts,
            unknown: &unknown,
            accessor,
        };
        let mut names = Names::default();
        for (i, e) in ast.exprs.iter().enumerate() {
            if dead_twins.contains(&ExprId(i as u32)) {
                continue;
            }
            let (hit, recv) = match e {
                Expr::Assign { target, value } => (
                    write(&facts, *target, Some(*value)),
                    any_receiver(ast, expr_types, *target),
                ),
                Expr::PostIncr { target, .. } => (
                    write(&facts, *target, None),
                    any_receiver(ast, expr_types, *target),
                ),
                Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
                    let reflect_set = matches!(ast.get_expr(*callee),
                        Expr::Member { obj, name } if name == "set"
                            && matches!(ast.get_expr(*obj), Expr::Ident(r) if r == "Reflect"));
                    let target = args.first().copied().filter(|_| reflect_set);
                    (call(ast, expr_types, *callee, args), target)
                }
                Expr::ObjectLit { fields } => (literal(expr_types, optional_number, fields), None),
                _ => (Hit::None, None),
            };
            names.take(spare(&origins, recv, hit));
        }
        if names.unknown.is_subset(&unknown) {
            return names;
        }
        unknown.extend(names.unknown);
    }
}

/// The receiver of a write to `target` the lowering makes through the
/// any lane: a member or keyed write whose receiver is no layout the
/// checker knows.
fn any_receiver(ast: &Ast, expr_types: &HashMap<ExprId, Type>, target: ExprId) -> Option<ExprId> {
    match ast.get_expr(target) {
        Expr::Member { obj, .. } | Expr::Index { obj, .. } if !struct_like(expr_types.get(obj)) => {
            Some(*obj)
        }
        _ => None,
    }
}

/// `hit` less what a write through `recv` cannot leave in any layout's
/// slot: the name, when the receiver never holds a struct cell laid out
/// with that slot, and every name, when it never holds one with any
/// slot (`receiver_origin`; `counts[w] = n` on a fresh dictionary). A
/// `Reflect.set` writes into its first argument; `Object.assign` names
/// the fields of its sources, which its target does not narrow here.
fn spare<'a>(
    origins: &dyn Fn() -> &'a super::receiver_origin::Origins<'a>,
    recv: Option<ExprId>,
    hit: Hit,
) -> Hit {
    match (recv, hit) {
        (Some(r), Hit::Name(n, _)) if origins().excludes(r, &n) => Hit::None,
        (Some(r), Hit::All(_)) if origins().holds_no_slot(r) => Hit::None,
        (_, hit) => hit,
    }
}

/// A write to `target`, with `value` its rhs (none for `++` / `--`,
/// whose result is a number unless the old value was a BigInt the
/// sidecar already holds).
fn write(facts: &super::write_kind::Facts<'_>, target: ExprId, value: Option<ExprId>) -> Hit {
    let (ast, expr_types) = (facts.ast, facts.expr_types);
    let kind = value.map_or(Kind::Number, |v| kind_of(expr_types.get(&v)));
    match ast.get_expr(target) {
        Expr::Member { obj, name } => {
            if !struct_like(expr_types.get(obj)) {
                if value.is_some_and(|v| super::write_kind::steps_itself(facts, v, *obj, name)) {
                    return Hit::None;
                }
                let number =
                    value.is_some_and(|v| super::write_kind::number_value(facts, v, *obj, name));
                let kind = if number { Kind::Number } else { kind };
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

/// The fields `Object.assign` copies out of `sources`, each with the
/// kind of its own value; a source whose fields are not all known
/// settles for every name.
fn assign_sources(ast: &Ast, expr_types: &HashMap<ExprId, Type>, sources: &[ExprId]) -> Hit {
    let mut names: Vec<(String, Kind)> = Vec::new();
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
            names.push((k.to_string(), kind_of(Some(t))));
        }
    }
    Hit::Names(names)
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
/// string, a collection or a buffer the key picks an element. A
/// dictionary (`Record<K, V>`) is a property receiver: the checker
/// admits no struct type into one, but an `any` value does
/// (`const d: Record<string, V> = p as any`), and the write then lands
/// on the struct cell.
fn property_receiver(t: Option<&Type>) -> bool {
    match t {
        None | Some(Type::Any | Type::Struct(_) | Type::ClassRef(_) | Type::Object(_)) => true,
        Some(Type::TypeVar(_) | Type::Dict(..)) => true,
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
            (&HashMap::new(), &HashMap::new()),
            &ShadowedNames::default(),
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
        // a fraction: an integer literal fits every number slot
        let n = settled("p[0] = 1.5; q[\"b\"] = 2.5;", |ast| {
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
    }

    #[test]
    fn a_dictionary_may_alias_a_struct_cell() {
        let dict = |v: Type| Type::Dict(Box::new(Type::String), Box::new(v));
        let n = settled("counts[w] = v;", |ast| {
            let mut t = ident_types(ast, "counts", dict(Type::Any));
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(n.may_shadow("a", true) && n.may_shadow("a", false));
        // a number keeps the f64 slots' bare loads
        let n = settled("counts[w] = n; d[\"s\"] = n;", |ast| {
            let mut t = ident_types(ast, "counts", dict(Type::Number));
            t.extend(ident_types(ast, "d", dict(Type::Number)));
            t.extend(ident_types(ast, "n", Type::Number));
            t
        });
        assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
        let n = settled("d[\"s\"] = v;", |ast| {
            let mut t = ident_types(ast, "d", dict(Type::Any));
            t.extend(ident_types(ast, "v", Type::Any));
            t
        });
        assert!(n.may_shadow("s", false) && !n.may_shadow("t", false));
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
        // several fields of mixed kinds: each name with its own
        let mixed = crate::check::Type::struct_of(vec![
            (PropKey::from("a"), Type::Number),
            (PropKey::from("s"), Type::String),
        ]);
        let n = settled("Object.assign(p, q, r);", |ast| {
            let mut t = ident_types(ast, "q", mixed.clone());
            t.extend(ident_types(ast, "r", src.clone()));
            t
        });
        assert!(!n.may_shadow("a", true) && n.may_shadow("a", false));
        assert!(n.may_shadow("s", true) && !n.may_shadow("b", true));
        assert!(!n.may_shadow("zz", false));
    }

    #[test]
    fn an_any_receiver_stepping_its_own_number_member_fits_the_slot() {
        let decl = "type P = { pos: number; start: number; s: string };";
        let any_o = |ast: &Ast| ident_types(ast, "o", Type::Any);
        let n = settled(
            &format!("{decl} o.pos = o.pos + 1; o.pos = o.pos - 2;"),
            any_o,
        );
        assert!(!n.may_shadow("pos", false));
        // a product may be -0, which an integer slot cannot hold
        let n = settled(&format!("{decl} o.pos = o.pos * 2;"), any_o);
        assert!(!n.may_shadow("pos", true) && n.may_shadow("pos", false));
        // another number member of the same receiver
        let n = settled(&format!("{decl} o.pos = o.start;"), any_o);
        assert!(!n.may_shadow("pos", true) && n.may_shadow("pos", false));
        // a member a layout declares a string is no number
        let n = settled(&format!("{decl} o.pos = o.s;"), any_o);
        assert!(n.may_shadow("pos", true));
        // nor is a member another write leaves holding any value
        let n = settled(
            &format!("{decl} o.start = v; o.pos = o.start + 1;"),
            |ast| {
                let mut t = any_o(ast);
                t.extend(ident_types(ast, "v", Type::Any));
                t
            },
        );
        assert!(n.may_shadow("pos", true) && n.may_shadow("start", true));
    }

    #[test]
    fn a_number_member_is_one_the_lowering_gives_a_number_slot() {
        // the parser writes a member's literal type as its base type's
        // annotation (`k: 1` declares `k: number`), and the slot follows it
        let s = |s: &str| Type::Literal(Lit::Str(s.into()));
        for checked in [
            Type::Number,
            Type::Literal(Lit::Num(1.0)),
            Type::Literal(Lit::Num(-2.5)),
            Type::String,
            s("a"),
            Type::Boolean,
            Type::Literal(Lit::Bool(true)),
        ] {
            let base = match &checked {
                Type::Literal(l) => l.base(),
                t => t.clone(),
            };
            let ann = match base {
                Type::Number => "number",
                Type::String => "string",
                _ => "boolean",
            };
            let slot = crate::ssa_lower_parse_type::parse_struct_field_type(
                ann,
                false,
                &HashMap::new(),
                &mut Vec::new(),
                &mut Vec::new(),
                &Default::default(),
                &mut Vec::new(),
                &mut HashMap::new(),
            );
            let number_slot = matches!(
                slot,
                crate::ssa::Type::F64 | crate::ssa::Type::I64 | crate::ssa::Type::I32
            );
            assert_eq!(
                ShadowedNames::number_member(&checked),
                number_slot,
                "{checked:?}"
            );
        }
    }

    #[test]
    fn the_checker_retypes_the_members_written_a_value_of_another_type() {
        let src = "x.b = v; x.c = 1.5; x.__state = v;";
        let tokens = crate::lexer::tokenize(src).expect("tokenize");
        let ast = crate::parser::parse(src, &tokens).expect("parse");
        let mut types = ident_types(&ast, "x", Type::Any);
        types.extend(ident_types(&ast, "v", Type::Any));
        let names = ShadowedNames::of(&ast, &types);
        assert!(names.may_mistype("b", true) && names.may_mistype("b", false));
        assert!(!names.may_mistype("c", true) && names.may_mistype("c", false));
        // a member the compiler mints keeps its type
        assert!(!names.may_mistype("__state", false));
    }

    #[test]
    fn a_receiver_that_never_holds_a_struct_cell_shadows_nothing() {
        let p = crate::check::Type::struct_of(vec![(PropKey::from("y"), Type::Number)]);
        let any_of = |names: &[&str]| {
            let p = p.clone();
            let names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
            move |ast: &Ast| {
                let mut t: HashMap<ExprId, Type> = HashMap::new();
                for n in &names {
                    t.extend(ident_types(ast, n, Type::Any));
                }
                t.extend(ident_types(ast, "p", p.clone()));
                t
            }
        };
        let n = settled(
            "const cfg: any = {}; cfg.x = v; cfg[\"w\"] = v; Reflect.set(cfg, \"u\", v);",
            any_of(&["cfg", "v"]),
        );
        assert!(!n.may_shadow("x", false) && !n.may_shadow("w", false));
        assert!(!n.may_shadow("u", false));
        // a JSON value, a literal without the key, a ternary of both
        let n = settled(
            "let j: any = JSON.parse(s); j.x = v; const t: any = c ? {} : { z: 1 }; t.x = v;",
            any_of(&["j", "t", "v"]),
        );
        assert!(!n.may_shadow("x", false));
        // a computed key on an empty literal reaches no slot; on a
        // literal with keys it may reach one
        let n = settled(
            "const counts: any = {}; counts[w] = v;",
            any_of(&["counts", "v", "w"]),
        );
        assert!(!n.may_shadow("a", false));
        let n = settled(
            "const d: any = { a: 1 }; d[k] = v;",
            any_of(&["d", "v", "k"]),
        );
        assert!(n.may_shadow("a", false) && n.may_shadow("b", false));
        // a literal with the key, an assigned struct, a parameter, a reviver
        for src in [
            "const o: any = { x: 1 }; o.x = v;",
            "let o: any = {}; o = p; o.x = v;",
            "function f(o: any) { o.x = v; }",
            "const o: any = JSON.parse(s, r); o.x = v;",
        ] {
            let n = settled(src, any_of(&["o", "v"]));
            assert!(n.may_shadow("x", false), "{src}");
        }
    }
}
