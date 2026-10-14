//! How the getter's answer becomes a value of the slot's type.
//!
//! The answer is an owned `any`, and the slot's type is what every
//! typed consumer of the read relies on, so the answer crosses only as
//! a value already of that type: any other value is a TypeError at the
//! read, never a silently converted one. A field whose annotation
//! admits `undefined` (`v?: number`, `s: string | undefined`) takes it
//! in the slot's own spelling (the F64 and Str sentinels).

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::check_type_ann::strip_nullable;
use crate::ssa::{FuncId, InstKind, Module, Operand, StructId, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};
use crate::ssa_lower_checked_cast::CastSlot;

/// `(any, site) -> f64`: a number, or the slot's undefined sentinel.
const NUMBER_OR_UNDEF: &str = "__torajs_obj_field_exotic_number_or_undef";
/// `(any, slot cell, site) -> owned ptr`: the cell the slot holds.
const SAME_CELL: &str = "__torajs_obj_field_exotic_same_cell";
/// `(any, site) -> ptr`: always the TypeError.
const REFUSE: &str = "__torajs_obj_field_exotic_refuse";

pub(super) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(
        module,
        fn_table,
        NUMBER_OR_UNDEF,
        &[Type::Any, Type::Str],
        Type::F64,
    );
    declare_intrinsic(
        module,
        fn_table,
        SAME_CELL,
        &[Type::Any, Type::Ptr, Type::Str],
        Type::Ptr,
    );
    declare_intrinsic(module, fn_table, REFUSE, &[Type::Any, Type::Str], Type::Ptr);
}

pub(super) enum Conv {
    /// `true` when the annotation admits `undefined`.
    Number(bool),
    Bool,
    Str(bool),
    Any,
    Obj(StructId, String),
    Arr(String),
    /// One heap family whose cells every lane spells alike.
    Cell(torajs_rc::Tag),
    /// A heap type whose typed lane has a layout or calling convention
    /// of its own (a closure's signature, a typed collection's value
    /// lane, a layout no cast plan describes): only the cell the slot
    /// already holds crosses, which is what an accessor answering the
    /// field's own value, or another field's redefinition, gives.
    SameCell,
    /// A raw function address: no any-lane value is one, so every
    /// answer is the TypeError rather than a call through the old slot.
    Refuse,
}

/// The crossing for a field of type `field_ty`, and the type its
/// TypeError names; `None` for a slot no accessor answer can reach as a
/// value of its type (a raw word that is no value at all), whose read
/// stays a bare load.
pub(super) fn conversion(
    ctx: &LowerCtx<'_>,
    obj: Option<ExprId>,
    name: &str,
    field_ty: Type,
) -> Option<(Conv, String)> {
    let ann =
        obj.and_then(|o| crate::ast_throw_info::class_field_ann(ctx.ast, ctx.expr_types, o, name));
    // an annotation the parser respelled (`__cls(..)`) names its type
    // by kind
    let base = (ann.as_deref().map(|a| strip_nullable(a).trim().to_string()))
        .filter(|b| !b.starts_with("__"));
    // an optional `number` member of an object type has no class
    // annotation to read; its checked type says the same thing
    let optional_number =
        crate::ssa_lower_object_lit::absent::optional_number_slot(ctx, obj, name, field_ty);
    let or_undef = ann.as_deref().is_some_and(|a| a.starts_with("__nullable(")) || optional_number;
    let shown = match (&base, or_undef) {
        (Some(b), true) => format!("{b} | undefined"),
        (Some(b), false) => b.clone(),
        (None, true) if field_ty == Type::F64 => "number | undefined".to_string(),
        (None, _) if matches!(field_ty, Type::Closure(_) | Type::FnSig(_)) => {
            "function".to_string()
        }
        (None, _) => format!("{field_ty:?}").to_lowercase(),
    };
    let conv = match field_ty {
        Type::I64 | Type::F64 => Conv::Number(or_undef),
        Type::Bool => Conv::Bool,
        Type::Str => Conv::Str(or_undef),
        Type::Any => Conv::Any,
        Type::Obj(sid) => match ann {
            Some(ann) if ctx.casts_obj_slot(sid, &ann) => Conv::Obj(sid, ann),
            _ => Conv::SameCell,
        },
        Type::Arr(_) => match ann {
            Some(ann) if ctx.casts_obj_arr_slot(field_ty, &ann) => Conv::Arr(ann),
            _ => Conv::SameCell,
        },
        Type::BigInt => Conv::Cell(torajs_rc::Tag::BigInt),
        Type::Date => Conv::Cell(torajs_rc::Tag::Date),
        Type::Symbol => Conv::Cell(torajs_rc::Tag::Symbol),
        Type::FnSig(_) => Conv::Refuse,
        t if t.is_refcounted() => Conv::SameCell,
        _ => return None,
    };
    Some((conv, shown))
}

/// `got` (owned) as a value of `field_ty`, owned or Copy; `got`'s
/// stake is settled here, on the throw path too. `slot` is the
/// instance and the field's offset.
pub(super) fn convert(
    ctx: &mut LowerCtx<'_>,
    conv: &Conv,
    got: Operand,
    field_ty: Type,
    (obj_val, offset): (Operand, u64),
    (name, shown): (&str, &str),
) -> Operand {
    if let Conv::Any = conv {
        return got;
    }
    let tok = ctx.push_throw_temp(got, Type::Any);
    let text = format!("cannot read {{}} through the field `{name}: {shown}`");
    let site = Operand::Value(ctx.intern_string_literal(text.as_str()));
    let v = match conv {
        Conv::Any => unreachable!("returned above"),
        Conv::Number(or_undef) => {
            let f = if *or_undef {
                ctx.fn_table[NUMBER_OR_UNDEF]
            } else {
                ctx.intrinsics.anyv_cast_number
            };
            let n = call(ctx, f, vec![got, site], Type::F64);
            if field_ty == Type::I64 {
                ctx.coerce_to_i64(n)
            } else {
                n
            }
        }
        Conv::Bool => call(
            ctx,
            ctx.intrinsics.anyv_cast_bool,
            vec![got, site],
            Type::Bool,
        ),
        Conv::Str(or_undef) => {
            let f = if *or_undef {
                ctx.intrinsics.anyv_cast_str_or_undef
            } else {
                ctx.intrinsics.anyv_cast_str
            };
            call(ctx, f, vec![got, site], Type::Str)
        }
        Conv::Obj(sid, ann) => (ctx.checked_cast_obj(got, *sid, ann, CastSlot::Assign))
            .expect("conversion() admitted only a slot the cast tests"),
        Conv::Arr(ann) => (ctx.checked_cast_obj_arr(got, field_ty, ann, CastSlot::Assign))
            .expect("conversion() admitted only a slot the cast tests"),
        Conv::Cell(tag) => {
            let tag = Operand::ConstI64(*tag as i64);
            let f = ctx.intrinsics.anyv_cast_cell;
            call(ctx, f, vec![got, tag, site], field_ty)
        }
        Conv::SameCell => {
            let slot = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Ptr, obj_val, offset),
                Type::Ptr,
                None,
            );
            let f = ctx.fn_table[SAME_CELL];
            call(ctx, f, vec![got, Operand::Value(slot), site], field_ty)
        }
        Conv::Refuse => {
            let f = ctx.fn_table[REFUSE];
            call(ctx, f, vec![got, site], field_ty)
        }
    };
    ctx.pop_throw_temp(tok);
    ctx.emit_drop_value(got, Type::Any);
    v
}

/// A kernel call answering `ret`, then its throw check.
fn call(ctx: &mut LowerCtx<'_>, f: FuncId, args: Vec<Operand>, ret: Type) -> Operand {
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(f, args), ret, None);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}
