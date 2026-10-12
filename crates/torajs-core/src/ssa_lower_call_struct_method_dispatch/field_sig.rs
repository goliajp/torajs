//! Which member calls name a callable field, moved out of [`super`]
//! whole.

use crate::ast::ExprId;
use crate::check::{self as check_mod};
use crate::ssa_lower::LowerCtx;

/// Does `recv.name` name a field that holds a callable, and is its
/// declared signature a rest-tail one? `Some(is_variadic)` admits the
/// call to this lane; `None` falls through.
///
/// Two receivers ask the same question. An inline struct spells its
/// shape in its own checked type, so the field is read straight out of
/// it. A class instance does not: RFC 20260715-nominal-class-identity
/// keeps it `ClassRef(C)` and puts the shape in a side table the
/// lowering has no view of — so this lane only ever saw the structural
/// one, and `c.f(3)` fell through to the `resolve_callee` panic
/// ("unsupported member call shape") while `const g = c.f; g(3)`
/// worked, which is the same value reached through a name. For a class
/// the field read's own recorded type answers it, without re-deriving
/// the class's shape here.
///
/// Both arms stay a hashmap lookup, so a miss still costs nothing and
/// the receiver is not lowered twice. The nominal receiver test is what
/// keeps builtin receivers out: `arr.push` is typed on an `Array`, and
/// its own lane is downstream of this one.
pub(super) fn callable_field_is_variadic(
    ctx: &LowerCtx<'_>,
    obj: ExprId,
    callee: ExprId,
    name: &str,
) -> Option<bool> {
    let field_check_ty = match ctx.expr_types.get(&obj)? {
        check_mod::Type::Struct(fields) => {
            fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)?
        }
        _ if crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, obj).is_some() => {
            ctx.expr_types.get(&callee)?
        }
        _ => return None,
    };
    // RFC 20260710 C5 — an optional fn field (`cb?: (n) => R`) is
    // declared Nullable(Function); the checker only admits the call
    // inside a truthiness-narrowed branch (`if (o.cb) { o.cb() }`),
    // so by here the callable shape is guaranteed — unwrap to reach
    // the signature. Un-narrowed calls never lower (loud reject).
    let field_check_ty = match field_check_ty {
        check_mod::Type::Nullable(inner) => inner.as_ref(),
        other => other,
    };
    let check_mod::Type::Function(field_params, _) = field_check_ty else {
        return None;
    };
    // RFC 20260708-variadic chunk 3 — a rest-tail field sig
    // (`fn: (...args: E[]) => R`) never static-dispatches: the SSA
    // sig only carries the fixed prefix, so the CallIndirect argv
    // would mismatch the stored closure's real arity (SIGSEGV).
    // Route through the boxed dual entry instead.
    Some(matches!(
        field_params.last(),
        Some(check_mod::Type::Rest(_))
    ))
}

/// Is `recv.name` a field of the property-slot census
/// (`Ast::face_slots`), which some store handed a function value of
/// another face? Its calls ride the boxed entry.
pub(super) fn is_face_slot_field(ctx: &LowerCtx<'_>, obj: ExprId, name: &str) -> bool {
    let census = &ctx.ast.face_slots;
    !census.fields.is_empty()
        && ctx
            .expr_types
            .get(&obj)
            .and_then(|t| check_mod::face_slots::owner_key(ctx.ast, t))
            .is_some_and(|owner| census.field_hit(&ctx.ast.class_parents, &owner, name))
}
