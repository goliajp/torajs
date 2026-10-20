//! The expression walk of the `then` closure proof: every variant named
//! in one match, each form that can open judged by its own function.

use std::collections::HashMap;

use super::forms::{
    builtin_global, danger_member, danger_string, helper_defines_nothing, ident_opens,
};
use super::keys::{call_opening, key_opening};
use super::{Opening, open};
use crate::ast::{Ast, Expr, ExprId};
use crate::check::Type;
use crate::check::awaited::settles_as_object;
use crate::ssa::id_hash::HashSet;

/// One expression, every variant named: a new one must be judged here.
pub(super) fn expr_opening(
    ast: &Ast,
    eid: ExprId,
    e: &Expr,
    vetted: &HashSet<ExprId>,
    types: Option<&HashMap<ExprId, Type>>,
) -> Option<Opening> {
    match e {
        Expr::Ident(n) => ident_opening(n, vetted.contains(&eid)),
        Expr::String(s) => s.as_wtf8().as_str().and_then(string_opening),
        Expr::Member { obj, name } | Expr::OptChain { obj, name } => {
            member_opening(ast, *obj, name)
        }
        Expr::Index { index, .. } | Expr::OptIndex { index, .. } => key_opening(ast, *index),
        Expr::Call { callee, args } | Expr::OptCall { callee, args } => {
            call_opening(ast, *callee, args)
        }
        Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => write_opening(ast, *target),
        Expr::ObjectLit { fields } => fields
            .iter()
            .find_map(|(k, _)| literal_member_opening(&k.lossy())),
        Expr::New { class_name, .. } if ident_opens(class_name) => open("new", class_name.clone()),
        Expr::As { expr, ty_ann } => cast_opening(types?, eid, *expr, ty_ann),
        Expr::New { .. }
        | Expr::NewDynamic { .. }
        | Expr::Number(_)
        | Expr::BigInt { .. }
        | Expr::Bool(_)
        | Expr::Uninit
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::BinOp { .. }
        | Expr::Unary { .. }
        | Expr::Array(_)
        | Expr::ArrowFn { .. }
        | Expr::Closure { .. }
        | Expr::This
        | Expr::NewTarget
        | Expr::Super { .. }
        | Expr::Ternary { .. }
        | Expr::Sequence { .. }
        | Expr::Elision
        | Expr::TypeOf { .. }
        | Expr::Delete { .. }
        | Expr::InstanceOf { .. }
        | Expr::Spread { .. }
        | Expr::Nullish { .. } => None,
    }
}

/// A name: a global that opens wherever it stands, a gated global out of
/// a form [`super::forms::vetted_roots`] cleared, or a compiler helper
/// that may define something.
fn ident_opening(n: &str, vetted: bool) -> Option<Opening> {
    // `new C(…)` reaches here as the class lowering's factory
    let ctor = n.strip_prefix("__new_").is_some_and(ident_opens);
    if ctor || ident_opens(n) || (super::forms::gated_root(n) && !vetted) {
        return open("global", n);
    }
    match n.strip_prefix("__torajs_") {
        Some(h) if !helper_defines_nothing(h) => open("helper", n),
        _ => None,
    }
}

/// A string literal that names `then`, the `__proto__` accessor or a
/// definer, wherever it stands.
fn string_opening(k: &str) -> Option<Opening> {
    if danger_string(k) {
        return open("string", k);
    }
    None
}

/// A member read: a dangerous name, or a builtin's `.prototype`.
fn member_opening(ast: &Ast, obj: ExprId, name: &str) -> Option<Opening> {
    if danger_member(name) {
        return open("member", name);
    }
    match ast.get_expr(obj) {
        Expr::Ident(n) if name == "prototype" && builtin_global(n) => {
            open("builtin prototype", n.clone())
        }
        _ => None,
    }
}

/// A write: to a member named `then` or `prototype`.
fn write_opening(ast: &Ast, target: ExprId) -> Option<Opening> {
    match ast.get_expr(target) {
        Expr::Member { name, .. } | Expr::OptChain { name, .. }
            if matches!(name.as_str(), "then" | "prototype") =>
        {
            open("write", name.clone())
        }
        _ => None,
    }
}

/// An object-literal member (accessors spelled `__getter_<k>` /
/// `__setter_<k>`): `then`, `__proto__` or a computed key.
fn literal_member_opening(k: &str) -> Option<Opening> {
    let bare = (k.strip_prefix("__getter_"))
        .or_else(|| k.strip_prefix("__setter_"))
        .unwrap_or(k);
    if bare == "then" || bare == "__proto__" || k.starts_with("__computed_") {
        return open("literal member", k);
    }
    None
}

/// An `as` cast: a promise or a value of another layout may travel
/// under the object type a cast of an `any` names. A generic body's
/// cast is judged by its specializations' copies, which the proof over
/// the mono program sees with their own types.
fn cast_opening(
    types: &HashMap<ExprId, Type>,
    eid: ExprId,
    operand: ExprId,
    ty_ann: &str,
) -> Option<Opening> {
    if types.get(&operand) == Some(&Type::Any) && types.get(&eid).is_some_and(settles_as_object) {
        return open("cast from any", ty_ann);
    }
    None
}
