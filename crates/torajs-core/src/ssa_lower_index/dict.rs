//! The keyed read and write on a dictionary the checker typed with
//! number values (`Record<string, number>`).
//!
//! The read still answers `any` (ES §10.1.8.1 [[Get]]: the key may
//! name an inherited member, and an `any` alias may have stored a
//! value of any type), so the type only says which outcome to try
//! first. Both accesses are one runtime call on NaN-box values
//! (`torajs-dynobj`'s `dict.rs`): an own entry holding a number is the
//! read's answer, an own writable entry holding an immediate takes
//! the write in place, and every other case runs inside the call
//! exactly as the generic pair read or member-set kernel would. The
//! call site is the call and its throw check, no branches of its own.

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Module, Operand, Type, ValueId};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

/// `__torajs_dynobj_dict_get(recv, key) -> any`, an owned answer.
const GET: &str = "__torajs_dynobj_dict_get";
/// `__torajs_dynobj_dict_set(recv_slot, key, value, throw_on_refusal)`,
/// the value's stake moving into the call.
const SET: &str = "__torajs_dynobj_dict_set";

/// Declares both entries in a program that has a number dictionary
/// (`Ast::number_dicts`); no other program calls them, so no other
/// program's SSA carries them.
pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    ast: &crate::ast::Ast,
) {
    if ast.number_dicts.is_empty() {
        return;
    }
    declare_intrinsic(module, fn_table, GET, &[Type::Any, Type::Ptr], Type::Any);
    declare_intrinsic(
        module,
        fn_table,
        SET,
        &[Type::Ptr, Type::Ptr, Type::Any, Type::Bool],
        Type::Void,
    );
}

/// `recv[key]` with `key` a borrowed Str cell, as an owned `any`.
pub(crate) fn get(ctx: &mut LowerCtx<'_>, recv: &Operand, key: ValueId) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.fn_table[GET], vec![*recv, Operand::Value(key)]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// The assigned value as a box carrying a stake of its own, for the
/// value types the write takes boxed (the encoding the member-set
/// kernel would store); `None` sends the write to the kernel.
pub(crate) fn box_value(
    ctx: &mut LowerCtx<'_>,
    value: ExprId,
    v_raw: &Operand,
    v_ty: Type,
) -> Option<Operand> {
    // `undefined` folds into the SSA null constant; the pair packer
    // tells the two apart by the checker type
    if matches!(
        ctx.expr_types.get(&value),
        Some(crate::check::Type::Undefined)
    ) {
        return None;
    }
    match v_ty {
        Type::Any if ctx.expr_transfers_ownership(value) => Some(*v_raw),
        // a borrowed inline view is copied out, as the owned unbox does
        Type::Any => Some(Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.anyv_retain, vec![*v_raw]),
            Type::Any,
            None,
        ))),
        Type::F64 | Type::I64 | Type::I32 => Some(ctx.box_to_any(*v_raw)),
        _ => None,
    }
}

/// `recv[key] = boxed` with `key` a borrowed Str cell and `recv` a
/// borrowed binding. The receiver rides a slot the way the kernel's
/// does, and an `any` local takes the slot back afterwards.
pub(crate) fn set(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    key: ValueId,
    boxed: Operand,
    recv_ident: &Option<String>,
) {
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(recv, Operand::Value(slot), 0),
    );
    let strict = !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx);
    let args = vec![
        Operand::Value(slot),
        Operand::Value(key),
        boxed,
        Operand::ConstBool(strict),
    ];
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(ctx.fn_table[SET], args));
    ctx.emit_throw_check(None);
    crate::ssa_lower_assign_member_any::write_back_recv(ctx, slot, recv_ident);
}
