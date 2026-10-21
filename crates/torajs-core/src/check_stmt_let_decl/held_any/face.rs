//! The type a read of a held `any` value has for the TypeScript rules.
//!
//! A binding that holds an `any` value (`super`, and the scalar form in
//! `super::super::any_scalar_slot`) types its reads `any` where the
//! lowering has to take the any lanes, but its declaration still says
//! what the program may do with it: tsc refuses `const s: string = n`
//! for `n: number` whatever `n` holds at run time. Such a refusal has no
//! runtime semantics, so it stays a compile-time error. [`face`] answers
//! the declared type of a read, and what an operator, a member read or
//! a call makes of one; the checks that compare a value with a slot ask
//! it whenever the value is typed `any`.

use crate::ast::{Ast, BinOp, Expr, ExprId, UnaryOp};
use crate::check::{Checker, DiagPush, Type};

/// The type `eid` has for the TypeScript rules when the checker typed it
/// `any` only because it reads a held value; `None` when it has no other.
pub(crate) fn face(c: &Checker, ast: &Ast, eid: ExprId) -> Option<Type> {
    let reads = &c.held_any.reads;
    if let Some(t) = reads.get(&eid) {
        return Some(t.clone());
    }
    if reads.is_empty() || c.expr_types.get(&eid) != Some(&Type::Any) {
        return None;
    }
    match ast.get_expr(eid) {
        Expr::BinOp { op, left, right } => binary(c, ast, *op, (*left, *right)),
        Expr::Unary {
            op: UnaryOp::Neg | UnaryOp::Plus | UnaryOp::BitNot,
            expr,
        } => face(c, ast, *expr).map(|_| Type::Number),
        Expr::Member { obj, name } => match face(c, ast, *obj)? {
            t @ (Type::Number | Type::String | Type::Boolean) => {
                crate::check_type_of_member::try_family_dispatch(&t, name)?.ok()
            }
            _ => None,
        },
        Expr::Call { callee, .. } => match face(c, ast, *callee)? {
            Type::Function(_, ret) => Some(*ret),
            _ => None,
        },
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            let (a, b) = sides(c, ast, (*then_branch, *else_branch))?;
            (a == b).then_some(a)
        }
        _ => None,
    }
}

/// Both operands' types, when at least one of them has a [`face`].
fn sides(c: &Checker, ast: &Ast, (l, r): (ExprId, ExprId)) -> Option<(Type, Type)> {
    let (fl, fr) = (face(c, ast, l), face(c, ast, r));
    if fl.is_none() && fr.is_none() {
        return None;
    }
    let l = fl.or_else(|| c.expr_types.get(&l).cloned())?;
    let r = fr.or_else(|| c.expr_types.get(&r).cloned())?;
    Some((l, r))
}

/// What `l op r` types as for the TypeScript rules (§13.15.3: `+` with
/// a string operand concatenates, the other arithmetic operators answer
/// a number).
fn binary(c: &Checker, ast: &Ast, op: BinOp, operands: (ExprId, ExprId)) -> Option<Type> {
    let (l, r) = sides(c, ast, operands)?;
    let numbers = l == Type::Number && r == Type::Number;
    match op {
        BinOp::Add if l == Type::String || r == Type::String => Some(Type::String),
        BinOp::Add
        | BinOp::Sub
        | BinOp::Mul
        | BinOp::Div
        | BinOp::Mod
        | BinOp::Pow
        | BinOp::BitAnd
        | BinOp::BitOr
        | BinOp::BitXor
        | BinOp::Shl
        | BinOp::Shr
        | BinOp::UShr => numbers.then_some(Type::Number),
        _ => None,
    }
}

/// The [`face`] of `eid` when it does not fit `slot`.
pub(crate) fn refused(c: &Checker, ast: &Ast, eid: ExprId, slot: &Type) -> Option<Type> {
    let f = face(c, ast, eid)?;
    let fits = crate::check_assignable::is_assignable_to_resolved(
        slot,
        &f,
        &c.class_structs,
        &c.aliases,
        &c.generic_alias_decls,
    );
    (!fits).then_some(f)
}

/// The argument `i` of a call, `arg`: refused when its [`face`] does not
/// fit the parameter type `param`.
pub(crate) fn check_arg(
    c: &Checker,
    ast: &Ast,
    (i, arg): (usize, ExprId),
    param: &Type,
) -> Result<(), String> {
    match refused(c, ast, arg, param) {
        Some(f) => Err(format!("argument {i}: expected {param:?}, got {f:?}")),
        None => Ok(()),
    }
}

/// A member `name` read through `obj`, a read of a held scalar: the
/// member must exist on the declared type, as on a typed receiver
/// (`n.toUpperCase()` with `n: number` is refused).
pub(crate) fn check_receiver(
    c: &Checker,
    ast: &Ast,
    (obj, name): (ExprId, &str),
    lenient_missing: bool,
) -> Result<(), String> {
    let Some(f @ (Type::Number | Type::String | Type::Boolean)) = face(c, ast, obj) else {
        return Ok(());
    };
    let shadowed = !c.proto_shadow.is_empty()
        && crate::builtin_proto_shadow::family_of(&f)
            .is_some_and(|family| c.proto_shadow.shadows(family, name));
    if name == "value" || shadowed {
        return Ok(());
    }
    match crate::check_type_of_member::try_family_dispatch(&f, name) {
        Some(r) => r.map(|_| ()),
        None => crate::check_type_of_member_terminal_miss::answer(
            ast,
            &obj,
            name,
            &f,
            &crate::check_type_of_member_terminal_miss::MissFacts {
                lenient_missing,
                spec_body: c.checking_spec_body,
                declared_private: c.declared_privates.contains(name),
            },
        )
        .map(|_| ()),
    }
}

/// [`check_receiver`] for a method call `callee` on a binding, whose
/// routes may answer from the callee's syntax without reading the
/// member: the receiver is typed here first, which records its face.
pub(crate) fn check_method_receiver(
    c: &mut Checker,
    ast: &Ast,
    callee: ExprId,
) -> Result<(), String> {
    let Expr::Member { obj, name } = ast.get_expr(callee) else {
        return Ok(());
    };
    if !c.held_any.scalars || !matches!(ast.get_expr(*obj), Expr::Ident(_)) {
        return Ok(());
    }
    if c.type_of(ast, *obj)? != Type::Any {
        return Ok(());
    }
    check_receiver(c, ast, (*obj, name), false)
}

/// A `let` / `const` initializer `init` of a binding `name` declared
/// `ann`: refused when its [`face`] does not fit the annotation.
pub(crate) fn refuses_init(
    c: &mut Checker,
    ast: &Ast,
    name: &str,
    init: ExprId,
    ann: &Type,
) -> bool {
    let Some(f) = refused(c, ast, init, ann) else {
        return false;
    };
    c.errors.push_err(format!(
        "type mismatch on `{name}`: declared {ann:?}, init has {f:?}"
    ));
    true
}

/// A store of `value` (typed `value_ty`) into the binding `name`, whose
/// type is `slot`, or `held` when it holds a box: checked here when
/// either side has a type the checker's own does not show.
pub(crate) fn check_store(
    c: &Checker,
    ast: &Ast,
    name: &str,
    (slot, held): (&Type, Option<Type>),
    (value, value_ty): (ExprId, &Type),
) -> Result<(), String> {
    let f = face(c, ast, value);
    if held.is_none() && f.is_none() {
        return Ok(());
    }
    let slot = held.as_ref().unwrap_or(slot);
    let shown = f.as_ref().unwrap_or(value_ty);
    if crate::check_assignable::is_assignable_to_resolved(
        slot,
        shown,
        &c.class_structs,
        &c.aliases,
        &c.generic_alias_decls,
    ) {
        Ok(())
    } else {
        Err(format!(
            "type mismatch assigning to `{name}`: declared {slot:?}, value is {shown:?}"
        ))
    }
}
