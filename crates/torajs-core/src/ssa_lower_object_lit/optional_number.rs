//! An optional `number` member's f64 slot: which slot is one, what a
//! write it cannot spell is, and the sentinel it holds for `undefined`.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Whether the member `field` of the receiver `obj` is an optional
/// `number` member kept in an f64 slot (`parse_struct_field_type`): its
/// checked type is `number | undefined`, and the slot is an f64 — a
/// member that may also hold `null` boxes, and a plain `number` member
/// is typed `number`. `None` when the receiver's type does not name the
/// member (an `any`-widened receiver, a layout the checker never saw):
/// the slot alone then has to answer, by name
/// ([`optional_number_slot`]).
pub(crate) fn optional_number_member(
    ctx: &LowerCtx<'_>,
    obj: ExprId,
    field: &str,
    slot_ty: Type,
) -> Option<bool> {
    use crate::check::Type as CType;
    if slot_ty != Type::F64 {
        return Some(false);
    }
    let fields = match ctx.expr_types.get(&obj) {
        Some(CType::Struct(fs)) => Some(fs),
        Some(CType::Nullable(inner)) => match &**inner {
            CType::Struct(fs) => Some(fs),
            _ => None,
        },
        _ => None,
    };
    match fields {
        Some(fs) => fs.iter().find(|(k, _)| *k == field).map(|(_, t)| match t {
            CType::Nullable(n) => **n == CType::Number,
            _ => false,
        }),
        // a class instance: the declaration's annotation
        None => crate::ast_throw_info::class_field_ann(ctx.ast, ctx.expr_types, obj, field)
            .map(|a| a == crate::ssa_lower_parse_type::OPTIONAL_NUMBER_ANN),
    }
}

/// Whether the f64 slot `field` of the receiver `obj` (none for a
/// whole-object reader with no receiver expression) is an optional
/// `number` member's: the receiver's type says when it names the
/// member, else the slot's name does (`optional_number_fields`), which
/// takes a same-named required `number` slot of a layout the checker
/// did not describe down the any lane — never the other way round.
pub(crate) fn optional_number_slot(
    ctx: &LowerCtx<'_>,
    obj: Option<ExprId>,
    field: &str,
    slot_ty: Type,
) -> bool {
    if slot_ty != Type::F64 {
        return false;
    }
    obj.and_then(|o| optional_number_member(ctx, o, field, slot_ty))
        .unwrap_or_else(|| {
            ctx.ast
                .accessor_field_names
                .optional_number_member(&PropKey::from(field))
        })
}

/// Whether the value `v` (the lowering of `value`) is one a number
/// slot of type `slot_ty` has no spelling for: an any-lane value, which
/// may be anything, or a `null`. The slot holds a double or, for an
/// optional member, the undefined sentinel; such a value goes to the
/// any lane's [[Set]], which stores a number and shadows the rest.
pub(crate) fn number_slot_cannot_spell(
    ctx: &LowerCtx<'_>,
    slot_ty: Type,
    v: Operand,
    value: ExprId,
) -> bool {
    if !matches!(slot_ty, Type::F64 | Type::I64) {
        return false;
    }
    match ctx.operand_ty(&v) {
        Type::Any => true,
        Type::Ptr => {
            matches!(v, Operand::ConstPtrNull)
                && !matches!(
                    ctx.expr_types.get(&value),
                    Some(crate::check::Type::Undefined)
                )
        }
        _ => false,
    }
}

/// An f64 slot's undefined: the sentinel every consumer of a `number`
/// field that may hold one reads.
pub(crate) fn f64_undef() -> Operand {
    Operand::ConstF64(f64::from_bits(
        crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
    ))
}
