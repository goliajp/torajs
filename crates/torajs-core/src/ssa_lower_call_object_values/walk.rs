//! Which receivers `Object.values` reads through the runtime own-walk
//! rather than the typed unfold. Moved out of [`super`].

use crate::ast::ExprId;
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Whether the values come from the boxed runtime own-walk rather than
/// the homogeneous fast path. S132 — heterogeneous fields route the
/// walk (the same Any-arm as an `any` receiver), which per-field
/// decodes via the field_metadata's type_tag. Two ways here: the
/// checker's rotation-545 `Array<Any>` verdict for a plain
/// heterogeneous (or empty) struct, and the older `<typed> as any`
/// spelling where check sees Type::Any but ssa-lower still sees
/// Type::Obj(sid) (`lower_as_cast` is a no-op for refcounted inner
/// types). Falling through the homogeneous fast path instead would
/// emit a wrong-type Load for every field — the str ptr read through
/// layout[0]=I64 returns the raw VA as a Number. An empty struct has no
/// elem type to narrow to and the walker answers its [] directly. A
/// member a write of the program can shadow answers the sidecar as an
/// any-lane value (`shadow_read`), which no typed element slot can
/// carry, so it takes the walk too, which reads the sidecar.
pub(super) fn runtime_walks(
    ctx: &LowerCtx<'_>,
    props: &[crate::ssa_lower_struct_own_props::OwnProp],
    recv: ExprId,
) -> bool {
    props.is_empty()
        || crate::ssa_lower_struct_own_props::some_member_shadowed(ctx, props, recv)
        || !props.iter().all(|p| p.ty() == props[0].ty())
}

/// Receivers the typed lane hands to the runtime own-values walk
/// boxed: a closure (its expando props), an Error-family instance
/// (`error_family_receiver`), and an instance of a class with a
/// runtime-computed field (629-03 — the checker answers `Array<Any>`
/// through the same `class_has_computed_instance_fields` verdict, so
/// the two lanes agree on the slot).
pub(super) fn boxes_for_own_walk(ctx: &LowerCtx<'_>, arg_op: &Operand, recv: ExprId) -> bool {
    if matches!(ctx.operand_ty(arg_op), Type::Closure(_))
        || crate::ssa_lower_call_object_entries::error_family_receiver(ctx, recv)
    {
        return true;
    }
    match ctx.expr_types.get(&recv) {
        Some(crate::check::Type::ClassRef(name)) => {
            crate::ast::class_has_computed_instance_fields(ctx.ast, name)
        }
        _ => false,
    }
}
