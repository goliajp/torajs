//! A call whose callee is a union of function types (`(c ? inc :
//! label)(x)`, or a binding holding such a choice).
//!
//! The callee's value travels as `any` and the call lowers through the
//! runtime any-call, whichever member runs. What the members still
//! decide is the check: the call is typed against the signature tsc
//! builds for a union (each argument position takes the intersection
//! of the members' parameters there, so an argument must fit every
//! member that reads it; the result is the union of the members'
//! returns). An argument no member could accept is refused here, as a
//! direct call refuses it: calling `inc("x")` has a runtime meaning,
//! but a call that is wrong for every member it may reach is the type
//! error TS reports (TS2345), not a program to admit.
//!
//! A nullish member is the runtime TypeError of §13.3.6.2 step 6 on
//! that arm, as for a `Nullable` function callee: the call is checked
//! against the callable members. A union holding any other kind of
//! member stays with the plain `any` callee route.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::{Checker, Type};

/// The function members of `t` when `t` is a union whose every member
/// is a function type or nullish, with at least one function.
pub(crate) fn fn_members(t: &Type) -> Option<Vec<(&[Type], &Type)>> {
    let Type::Union(ms) = t else {
        return None;
    };
    let mut sigs = Vec::new();
    for m in ms {
        match m {
            Type::Function(ps, r) => sigs.push((ps.as_slice(), r.as_ref())),
            Type::Null | Type::Undefined => {}
            _ => return None,
        }
    }
    (!sigs.is_empty()).then_some(sigs)
}

/// Claims a non-member call whose callee types as `any` and carries a
/// function union. `None` leaves the call to the `any` callee route.
pub(super) fn try_route(
    checker: &mut Checker,
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Result<Type, String>> {
    if matches!(ast.get_expr(callee), Expr::Member { .. })
        || !matches!(checker.type_of(ast, callee), Ok(Type::Any))
    {
        return None;
    }
    // a loose binding may hold any one member, which the union no
    // longer says: TS would have narrowed it
    if let Expr::Ident(n) = ast.get_expr(callee)
        && checker.is_loose(n)
    {
        return None;
    }
    let union = checker.unions.types.get(&callee)?.clone();
    let sigs = fn_members(&union)?;
    Some(check(checker, ast, &sigs, args))
}

fn check(
    checker: &mut Checker,
    ast: &Ast,
    sigs: &[(&[Type], &Type)],
    args: &[ExprId],
) -> Result<Type, String> {
    for (i, a) in args.iter().enumerate() {
        let lane = checker.type_of(ast, *a)?;
        let given = match ast.get_expr(*a) {
            Expr::Ident(n) if checker.is_loose(n) => lane,
            _ => checker.unions.types.get(a).cloned().unwrap_or(lane),
        };
        let slots: Vec<&Type> = sigs.iter().filter_map(|(ps, _)| param_at(ps, i)).collect();
        // an argument past every member's parameters binds nothing
        // (§10.2.1.4); it is still evaluated and typed
        if slots.is_empty() || admits(checker, &slots, &given) {
            continue;
        }
        return Err(format!(
            "argument {i}: {given:?} is not assignable to the parameter of every member {slots:?}"
        ));
    }
    // a missing argument is not refused: a function type does not say
    // which parameters are optional (`m: number = 2` is a `Number`
    // slot), and §10.2.1.4 binds undefined to the ones left out
    let ret = Type::union_of(sigs.iter().map(|(_, r)| match r {
        Type::Void => Type::Undefined,
        r => (*r).clone(),
    }));
    // the value comes back boxed from the any-call: a result whose
    // lane is not `any` (every member returns one type) would name a
    // representation the call does not produce
    Ok(if ret.clone().lane() == Type::Any {
        ret
    } else {
        Type::Any
    })
}

/// The parameters before a trailing rest sentinel.
fn fixed(ps: &[Type]) -> &[Type] {
    match ps.last() {
        Some(Type::Rest(_)) => &ps[..ps.len() - 1],
        _ => ps,
    }
}

/// The parameter a member binds argument `i` to, if any.
fn param_at(ps: &[Type], i: usize) -> Option<&Type> {
    let f = fixed(ps);
    match (f.get(i), ps.last()) {
        (Some(p), _) => Some(p),
        (None, Some(Type::Rest(e))) => Some(e),
        _ => None,
    }
}

/// Whether `given` fits the intersection of `slots`. An `any` slot
/// makes the intersection `any`; an `any` argument fits every
/// intersection but an empty one.
fn admits(checker: &Checker, slots: &[&Type], given: &Type) -> bool {
    if slots.iter().any(|s| matches!(s, Type::Any)) {
        return true;
    }
    if *given == Type::Any {
        return !disjoint(slots);
    }
    slots.iter().all(|s| assignable(checker, s, given))
}

fn assignable(checker: &Checker, to: &Type, from: &Type) -> bool {
    *to == Type::Any
        || crate::check_assignable::is_assignable_to_resolved(
            to,
            from,
            &checker.class_structs,
            &checker.aliases,
            &checker.generic_alias_decls,
        )
}

/// Whether the slots are primitive types with no value in common
/// (`number & string` is `never`).
fn disjoint(slots: &[&Type]) -> bool {
    let mut common: Option<Vec<Type>> = None;
    for s in slots {
        let Some(kinds) = primitive_kinds(s) else {
            return false;
        };
        common = Some(match common {
            None => kinds,
            Some(c) => c.into_iter().filter(|k| kinds.contains(k)).collect(),
        });
    }
    common.is_some_and(|c| c.is_empty())
}

/// The primitive types whose values make up `t`, or `None` when `t`
/// holds an object or an unknown value.
fn primitive_kinds(t: &Type) -> Option<Vec<Type>> {
    Some(match t {
        Type::Number | Type::String | Type::Boolean | Type::BigInt | Type::Symbol => {
            vec![t.clone()]
        }
        Type::Null | Type::Undefined => vec![t.clone()],
        Type::Literal(l) => vec![l.base()],
        Type::Nullable(inner) => {
            let mut k = primitive_kinds(inner)?;
            k.extend([Type::Null, Type::Undefined]);
            k
        }
        Type::Union(ms) => {
            let mut k = Vec::new();
            for m in ms {
                k.extend(primitive_kinds(m)?);
            }
            k
        }
        _ => return None,
    })
}
