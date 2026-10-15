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

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::accessor_names::{AccessorFieldNames, Reach};
use super::write_hit::{Hit, Kind, any_receiver, call, literal, spare, write};
use super::write_sites::candidates;
use crate::ast::{Ast, Expr, ExprId, PropKey};
use crate::check::{GenericAliasMap, Lit, Type};
use crate::ssa_lower_pass_0_5::arr_head_census::TwinBodies;

/// The names a pass reaches. `unknown` holds every name written a
/// value that is no number, which may shadow any slot but a number
/// slot; `any`, `string` and `boolean` split it by what the value was.
#[derive(Default)]
struct Names {
    unknown: HashSet<String>,
    number: HashSet<String>,
    any: HashSet<String>,
    string: HashSet<String>,
    boolean: HashSet<String>,
    all_unknown: bool,
    all_number: bool,
}

impl Names {
    fn take(&mut self, hit: &Hit) {
        match hit {
            Hit::None => {}
            Hit::Name(n, k) => self.name(n, *k),
            Hit::Names(ns) => {
                for (n, k) in ns {
                    self.name(n, *k);
                }
            }
            Hit::All(Kind::Number) => self.all_number = true,
            Hit::All(_) => self.all_unknown = true,
        }
    }

    fn name(&mut self, n: &str, k: Kind) {
        let by_kind = match k {
            Kind::Number => &mut self.number,
            Kind::String => &mut self.string,
            Kind::Boolean => &mut self.boolean,
            Kind::Unknown => &mut self.any,
        };
        by_kind.insert(n.to_string());
        if k != Kind::Number {
            self.unknown.insert(n.to_string());
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
        ast.accessor_field_names.shadowed.mistyped = mistyped.clone();
        let (optional_number, typed_optional) =
            super::optional_number_fields::names(ast, expr_types);
        ast.accessor_field_names.shadowed.optional_number = optional_number.clone();
        if matches!(ast.accessor_field_names.reach, Reach::All) {
            ast.accessor_field_names.shadowed.shadow = Reach::All;
            return;
        }
        let fields = &ast.accessor_field_names;
        let mut names = writes(
            ast,
            expr_types,
            &optional_number,
            &|n| fields.may(n),
            &ProgramWrites::default(),
        );
        ast.accessor_field_names.shadowed.found = ShadowedNames::of_names(&names);
        // a cast reaches no optional member in a program that has none
        if typed_optional || !ast.optional_members.none() {
            let resolve = (aliases, generic_aliases);
            let out = cast_reach(ast, expr_types, resolve, &optional_number, &mut names);
            ast.accessor_field_names.shadowed.cast_leaves_members_out = out;
        }
        ast.accessor_field_names.shadowed.shadow = Names::reach(names.all_unknown, names.unknown);
        ast.accessor_field_names.shadowed.shadow_number =
            Names::reach(names.all_number, names.number);
    }
}

/// What the program's casts from the any lane add to `names`: the
/// optional `number` members a cast can write a `null` into; and
/// whether a cast can leave an optional member out of a cell.
fn cast_reach(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    resolve: (&HashMap<String, Type>, &GenericAliasMap),
    optional_number: &HashSet<PropKey>,
    names: &mut Names,
) -> bool {
    let casts = super::cast_targets::names(ast, expr_types, resolve, optional_number);
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
            resolve,
            optional_number,
            &reached,
        );
        leaves_members_out |= conv.optional;
        if conv.number_names.is_subset(&names.unknown) {
            return leaves_members_out;
        }
        names.unknown.extend(conv.number_names);
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
    /// Written a string.
    pub string: HashSet<String>,
    /// Written a boolean.
    pub boolean: HashSet<String>,
}

/// What a member's slot holds as it is: a value of that kind written
/// through the any lane lands in the slot (`struct_data_field_set`), a
/// value of any other kind in the sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotKind {
    Number,
    String,
    Boolean,
    /// A slot no write is known to fit.
    Other,
}

impl SlotKind {
    /// The slot of a member of the checked type `t`: a `number`,
    /// `string` or `boolean` member, or one of a literal type (`k: 1`),
    /// whose slot is its base type's. The lowering reads the same answer
    /// off the slot's type ([`SlotKind::of_slot`]); the two have to
    /// agree, or the checker retypes a read the lowering keeps bare.
    pub fn of_member(t: &Type) -> SlotKind {
        match t {
            Type::Number | Type::Literal(Lit::Num(_)) => SlotKind::Number,
            Type::String | Type::Literal(Lit::Str(_)) => SlotKind::String,
            Type::Boolean | Type::Literal(Lit::Bool(_)) => SlotKind::Boolean,
            _ => SlotKind::Other,
        }
    }

    /// The kind of a layout slot of the lowering's type `t`.
    pub fn of_slot(t: crate::ssa::Type) -> SlotKind {
        use crate::ssa::Type as S;
        match t {
            S::F64 | S::I64 | S::I32 => SlotKind::Number,
            S::Str | S::Substr => SlotKind::String,
            S::Bool => SlotKind::Boolean,
            _ => SlotKind::Other,
        }
    }
}

impl ShadowedNames {
    pub fn of(ast: &Ast, expr_types: &HashMap<ExprId, Type>) -> Self {
        ShadowedNames::of_shared(ast, expr_types, &ProgramWrites::default())
    }

    /// [`ShadowedNames::of`] under one of the runs of a fixpoint over
    /// `ast`, which share what `program` reads off the program alone.
    pub(crate) fn of_shared<'a>(
        ast: &'a Ast,
        expr_types: &HashMap<ExprId, Type>,
        program: &ProgramWrites<'a>,
    ) -> Self {
        let settled = OnceCell::new();
        let accessor = |n: &str| {
            (settled.get_or_init(|| ast.accessor_field_names.may_settled(ast, expr_types)))(n)
        };
        let names = writes(ast, expr_types, &HashSet::new(), &accessor, program);
        ShadowedNames::of_names(&names)
    }

    /// The user-named members a pass reached, by the kind of the value.
    fn of_names(names: &Names) -> Self {
        // a member the compiler mints never mistypes (`may_mistype`)
        let user = |ns: &HashSet<String>| {
            (ns.iter().filter(|n| !n.starts_with("__")))
                .cloned()
                .collect()
        };
        ShadowedNames {
            unknown: user(&names.any),
            number: user(&names.number),
            string: user(&names.string),
            boolean: user(&names.boolean),
        }
    }

    pub fn is_subset(&self, other: &ShadowedNames) -> bool {
        self.unknown.is_subset(&other.unknown)
            && self.number.is_subset(&other.number)
            && self.string.is_subset(&other.string)
            && self.boolean.is_subset(&other.boolean)
    }

    pub fn extend(&mut self, other: ShadowedNames) {
        self.unknown.extend(other.unknown);
        self.number.extend(other.number);
        self.string.extend(other.string);
        self.boolean.extend(other.boolean);
    }

    /// Whether the member `name`, whose slot is a `slot` one, may hold a
    /// value of another type. A member the compiler mints (`__state`,
    /// `__sent`, an awaited value's slot) is read by the code it
    /// generates, which casts what the protocol hands it; its reads keep
    /// their type and their guard.
    pub fn may_mistype(&self, name: &str, slot: SlotKind) -> bool {
        let other = |kind, names: &HashSet<String>| slot != kind && names.contains(name);
        !name.starts_with("__")
            && (self.unknown.contains(name)
                || other(SlotKind::Number, &self.number)
                || other(SlotKind::String, &self.string)
                || other(SlotKind::Boolean, &self.boolean))
    }
}

/// What the census reads off the program alone, whatever the types:
/// the expressions that may write a member, and the twin bodies. The
/// runs of one fixpoint over the program share it.
#[derive(Default)]
pub(crate) struct ProgramWrites<'a> {
    writes: OnceCell<Vec<ExprId>>,
    twins: OnceCell<TwinBodies<'a>>,
}

/// Every write of the program, by the names it reaches. A
/// receiver-polymorphic twin's writes count like any other once the
/// twin can run: it types its receiver, parameters and locals `any`, so
/// its `t.m = v` is an any-lane [[Set]] onto whatever cell the caller
/// handed it, and the value is a number only when `write_kind` shows it
/// is one. A twin nothing enters (`TwinBodies::idle`) writes nothing.
/// That verdict reads which members the writes leave holding values of
/// any kind, so the census repeats while the set grows by a member some
/// verdict of the pass asked about. The receiver, the twin and the
/// layout facts a write needs are gathered on the first write that asks
/// for them.
fn writes<'a>(
    ast: &'a Ast,
    expr_types: &HashMap<ExprId, Type>,
    optional_number: &HashSet<PropKey>,
    accessor: &dyn Fn(&str) -> bool,
    program: &ProgramWrites<'a>,
) -> Names {
    // a literal writes only an optional `number` member
    let with_literals;
    let sites: &[ExprId] = match optional_number.is_empty() {
        true => program.writes.get_or_init(|| candidates(ast, false)),
        false => {
            with_literals = candidates(ast, true);
            &with_literals
        }
    };
    if sites.is_empty() {
        return Names::default();
    }
    // built only when some write's value needs its number-ness judged
    let layouts = OnceCell::new();
    // traced only when some write names a member through the any lane
    let origins = OnceCell::new();
    let origins = || origins.get_or_init(|| super::receiver_origin::Origins::of(ast, expr_types));
    // settled only when a write inside some twin names a member
    let dead_twins = OnceCell::new();
    let dead = |id: ExprId| {
        let twins = program.twins.get_or_init(|| TwinBodies::of(ast));
        twins.in_twin(id) && (dead_twins.get_or_init(|| twins.idle(expr_types))).contains(id)
    };
    // the verdict of each site, and the members it asked about while
    // `unknown` left them out: only a site that asked about a member a
    // pass adds can answer otherwise under the grown set
    let mut unknown: HashSet<String> = HashSet::new();
    let mut hits: Vec<Hit> = (0..sites.len()).map(|_| Hit::None).collect();
    let mut asked_by: Vec<Vec<String>> = vec![Vec::new(); sites.len()];
    let mut redo: Vec<usize> = (0..sites.len()).collect();
    loop {
        for &k in &redo {
            let asked = std::cell::RefCell::new(Vec::new());
            let facts = super::write_kind::Facts {
                ast,
                expr_types,
                layouts: &layouts,
                unknown: &unknown,
                asked: &asked,
                accessor,
                optional_number,
            };
            hits[k] = verdict(&facts, &origins, &dead, sites[k]);
            asked_by[k] = asked.into_inner();
        }
        let mut names = Names::default();
        for hit in &hits {
            names.take(hit);
        }
        let added: Vec<&String> = (names.unknown.iter())
            .filter(|n| !unknown.contains(*n))
            .collect();
        redo = (0..sites.len())
            .filter(|k| asked_by[*k].iter().any(|n| added.contains(&n)))
            .collect();
        if redo.is_empty() {
            return names;
        }
        unknown.extend(added.into_iter().cloned());
    }
}

/// What the write site `id` reaches under `facts`: nothing when its
/// receiver spares the name or the site sits in an idle twin.
fn verdict<'o>(
    facts: &super::write_kind::Facts<'_, '_>,
    origins: &dyn Fn() -> &'o super::receiver_origin::Origins<'o>,
    dead: &dyn Fn(ExprId) -> bool,
    id: ExprId,
) -> Hit {
    let (ast, expr_types) = (facts.ast, facts.expr_types);
    let e = ast.get_expr(id);
    let (hit, target) = match e {
        Expr::Assign { target, value } => (write(facts, *target, Some(*value)), Some(*target)),
        Expr::PostIncr { target, .. } => (write(facts, *target, None), Some(*target)),
        Expr::Delete { expr } => (super::delete_hit::hit(facts, *expr), Some(*expr)),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            (call(ast, expr_types, *callee, args), None)
        }
        Expr::ObjectLit { fields } => (literal(expr_types, facts.optional_number, fields), None),
        _ => (Hit::None, None),
    };
    if matches!(hit, Hit::None) {
        return Hit::None;
    }
    let recv = match (e, target) {
        (_, Some(t)) => any_receiver(ast, expr_types, t),
        (Expr::Call { callee, args } | Expr::OptCall { callee, args }, None) => args
            .first()
            .copied()
            .filter(|_| super::delete_hit::reflect_write(ast, *callee)),
        _ => None,
    };
    let hit = spare(origins, recv, hit);
    if matches!(hit, Hit::None) || dead(id) {
        return Hit::None;
    }
    hit
}

#[cfg(test)]
mod tests;
