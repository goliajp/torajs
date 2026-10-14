//! What one write of the program reaches: the member names a store,
//! a reflective or bulk write, or an object literal can leave holding a
//! value of another kind, each with the kind of its value
//! (`super::shadow_names` sums them over the program).

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, Expr, ExprId, PropKey};
use crate::check::{Lit, Type};

/// What a written value may be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A number: it fits an f64 slot and shadows every other.
    Number,
    /// A string: it fits a string slot and shadows every other.
    String,
    /// A boolean: it fits a boolean slot and shadows every other.
    Boolean,
    /// Anything: it may shadow any slot.
    Unknown,
}

pub(super) enum Hit {
    None,
    Name(String, Kind),
    /// Several names, each with the kind of its own value.
    Names(Vec<(String, Kind)>),
    All(Kind),
}

/// The receiver of a write to `target` the lowering makes through the
/// any lane: a member or keyed write whose receiver is no layout the
/// checker knows.
pub(super) fn any_receiver(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    target: ExprId,
) -> Option<ExprId> {
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
pub(super) fn spare<'a>(
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

/// The expressions that may write a member: a store or a step through
/// a member or a key, a call spelled `Reflect.set` / `Object.assign`,
/// and, with `literals`, an object literal (an expression of any other
/// kind reaches no name, whatever its type). A generic body's writes
/// are left out: it never lowers, the checker types nothing in it, and
/// each specialization the mono pass clones from it is a body of its
/// own, typed and counted in the census over the mono program.
pub(super) fn candidates(ast: &Ast, literals: bool) -> Vec<ExprId> {
    let member = |t: &ExprId| matches!(ast.get_expr(*t), Expr::Member { .. } | Expr::Index { .. });
    let generic = generic_bodies(ast);
    (ast.exprs.iter().enumerate())
        .filter(|(i, e)| {
            !generic.get(*i).is_some_and(|g| *g)
                && match e {
                    Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => member(target),
                    Expr::Call { callee, .. } | Expr::OptCall { callee, .. } => bulk(ast, *callee),
                    Expr::ObjectLit { .. } => literals,
                    _ => false,
                }
        })
        .map(|(i, _)| ExprId(i as u32))
        .collect()
}

/// One flag per arena slot: the expression sits in the body (or a
/// parameter default) of a function declared with type parameters.
/// Empty when the program declares none.
fn generic_bodies(ast: &Ast) -> Vec<bool> {
    use crate::ast::Stmt;
    use crate::ast::desugar_async_sm_walk::{visit_expr_children, visit_stmt_exprs};
    fn mark(ast: &Ast, e: ExprId, out: &mut [bool]) {
        if !std::mem::replace(&mut out[e.0 as usize], true) {
            visit_expr_children(ast, e, &mut |c| mark(ast, c, out));
        }
    }
    fn walk(ast: &Ast, s: &Stmt, out: &mut Vec<bool>) {
        match s {
            Stmt::FnDecl {
                type_params,
                params,
                body,
                ..
            } if !type_params.is_empty() => {
                if out.is_empty() {
                    *out = vec![false; ast.exprs.len()];
                }
                let mut add = |e: ExprId| mark(ast, e, out);
                for p in params {
                    if let Some(d) = p.default {
                        add(d);
                    }
                }
                for st in body {
                    visit_stmt_exprs(st, &mut add);
                }
            }
            Stmt::Multi(v) | Stmt::Block(v) => {
                for st in v {
                    walk(ast, st, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in &ast.stmts {
        walk(ast, s, &mut out);
    }
    out
}

/// A callee spelled `Reflect.set` or `Object.assign` ([`call`]).
fn bulk(ast: &Ast, callee: ExprId) -> bool {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return false;
    };
    matches!(ast.get_expr(*obj), Expr::Ident(b)
        if (b == "Reflect" && name == "set") || (b == "Object" && name == "assign"))
}

/// A write to `target`, with `value` its rhs (none for `++` / `--`,
/// whose result is a number unless the old value was a BigInt the
/// sidecar already holds).
pub(super) fn write(
    facts: &super::write_kind::Facts<'_, '_>,
    target: ExprId,
    value: Option<ExprId>,
) -> Hit {
    let (ast, expr_types) = (facts.ast, facts.expr_types);
    let kind = || value.map_or(Kind::Number, |v| value_kind(ast, expr_types, v));
    match ast.get_expr(target) {
        Expr::Member { obj, name } => {
            if !struct_like(expr_types.get(obj)) {
                if value.is_some_and(|v| super::write_kind::steps_itself(facts, v, *obj, name)) {
                    return Hit::None;
                }
                let number =
                    value.is_some_and(|v| super::write_kind::number_value(facts, v, *obj, name));
                let kind = if number { Kind::Number } else { kind() };
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
            let kind = kind();
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
pub(super) fn call(
    ast: &Ast,
    expr_types: &HashMap<ExprId, Type>,
    callee: ExprId,
    args: &[ExprId],
) -> Hit {
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
                .map_or(Kind::Unknown, |v| value_kind(ast, expr_types, *v));
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
pub(super) fn literal(
    expr_types: &HashMap<ExprId, Type>,
    optional_number: &HashSet<PropKey>,
    fields: &[(PropKey, ExprId)],
) -> Hit {
    if optional_number.is_empty() {
        return Hit::None;
    }
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
        Some(Type::String | Type::Literal(Lit::Str(_))) => Kind::String,
        Some(Type::Boolean | Type::Literal(Lit::Bool(_))) => Kind::Boolean,
        _ => Kind::Unknown,
    }
}

/// What the written value `v` may be: what its type says, else what its
/// own shape guarantees whatever its operands hold, which is what a
/// value the checker types `any` (a twin's, a state machine's) still
/// has. A `+` with a string operand concatenates (§13.15.3 step 1.c), a
/// `typeof` names a type; a comparison, a negation, an `instanceof` and
/// a `delete` answer a boolean.
fn value_kind(ast: &Ast, expr_types: &HashMap<ExprId, Type>, v: ExprId) -> Kind {
    use crate::ast::{BinOp, UnaryOp};
    let string = |e: &ExprId| {
        matches!(ast.get_expr(*e), Expr::String(_))
            || matches!(kind_of(expr_types.get(e)), Kind::String)
    };
    match (kind_of(expr_types.get(&v)), ast.get_expr(v)) {
        (Kind::Unknown, Expr::String(_) | Expr::TypeOf { .. }) => Kind::String,
        (Kind::Unknown, Expr::BinOp { op, left, right }) => match op {
            BinOp::Add if string(left) || string(right) => Kind::String,
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => Kind::Boolean,
            BinOp::Eq | BinOp::Neq | BinOp::LooseEq | BinOp::LooseNeq => Kind::Boolean,
            _ => Kind::Unknown,
        },
        (Kind::Unknown, Expr::Bool(_) | Expr::InstanceOf { .. } | Expr::Delete { .. }) => {
            Kind::Boolean
        }
        (
            Kind::Unknown,
            Expr::Unary {
                op: UnaryOp::Not, ..
            },
        ) => Kind::Boolean,
        (k, _) => k,
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
