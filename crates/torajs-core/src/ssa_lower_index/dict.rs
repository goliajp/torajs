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
/// `__torajs_dynobj_dict_get_entry(recv, key, entry_out, write_recv,
/// write_key) -> any`, the read that also answers the entry its write
/// may overwrite ([`super::dict_entry`]).
const GET_ENTRY: &str = "__torajs_dynobj_dict_get_entry";
/// `__torajs_dynobj_dict_set_entry(recv_slot, key, value,
/// throw_on_refusal, entry_slot)`, the write after that read: the
/// store into the answered entry, else the plain write.
const SET_ENTRY: &str = "__torajs_dynobj_dict_set_entry";

/// Declares the entries in a program that has a number dictionary
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
        GET_ENTRY,
        &[Type::Any, Type::Ptr, Type::Ptr, Type::Any, Type::Ptr],
        Type::Any,
    );
    declare_intrinsic(
        module,
        fn_table,
        SET,
        &[Type::Ptr, Type::Ptr, Type::Any, Type::Bool],
        Type::Void,
    );
    declare_intrinsic(
        module,
        fn_table,
        SET_ENTRY,
        &[Type::Ptr, Type::Ptr, Type::Any, Type::Bool, Type::Ptr],
        Type::Void,
    );
}

/// `recv[key]` with `key` a borrowed Str cell, as an owned `any`; the
/// read `index` keys also answers its entry when the write around it
/// asked ([`super::dict_entry`]).
pub(crate) fn get(ctx: &mut LowerCtx<'_>, recv: &Operand, index: ExprId, key: ValueId) -> Operand {
    let mut args = vec![*recv, Operand::Value(key)];
    let callee = match super::dict_entry::answer(ctx, index) {
        Some(more) => {
            args.extend(more);
            ctx.fn_table[GET_ENTRY]
        }
        None => ctx.fn_table[GET],
    };
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(callee, args), Type::Any, None);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// Ask the field read `index` keying a read of a number dictionary to
/// answer borrowed: a read behind the exotic-field guard otherwise
/// takes a stake on its plain arm only for this call to release it.
///
/// The borrow is sound because the key's last use is the call itself:
/// nothing is evaluated between the field load and the call, the
/// own-entry path runs no code of the program, and the generic half,
/// the only path that can run a getter or a proxy trap (which may
/// rewrite the field or release the record), pins the key first
/// (`torajs-dynobj`'s `dict.rs`). A write keeps its stake: its
/// right-hand side runs between the key read and the store.
pub(crate) fn ask_key_borrow(ctx: &mut LowerCtx<'_>, index: ExprId) -> bool {
    let asked = matches!(ctx.ast.get_expr(index), crate::ast::Expr::Member { .. });
    if asked {
        ctx.binop.key_borrow = Some((index, None));
    }
    asked
}

/// Whether the field read `eid` of a `slot_ty` slot is the key
/// [`ask_key_borrow`] asked to answer borrowed.
pub(crate) fn key_borrow_asked(ctx: &LowerCtx<'_>, eid: ExprId, slot_ty: Type) -> bool {
    slot_ty == Type::Str && ctx.binop.key_borrow == Some((eid, None))
}

/// The read takes the request: the flag slot its arms write (true where
/// the getter's answer carries a stake).
pub(crate) fn answer_key_borrow(ctx: &mut LowerCtx<'_>) -> ValueId {
    let flag = ctx.alloca_in_entry(Type::Bool, Some("__key_owned"));
    if let Some((_, answer)) = &mut ctx.binop.key_borrow {
        *answer = Some(flag);
    }
    flag
}

/// The flag slot when the read answered the request.
pub(crate) fn key_borrow_answer(ctx: &mut LowerCtx<'_>, asked: bool) -> Option<ValueId> {
    if !asked {
        return None;
    }
    ctx.binop.key_borrow.take().and_then(|(_, answer)| answer)
}

/// [`get`], then, for a borrowed key read, the release of the stake
/// its hit arm took.
pub(crate) fn get_then_settle(
    ctx: &mut LowerCtx<'_>,
    recv: &Operand,
    index: ExprId,
    key: ValueId,
    borrowed: Option<ValueId>,
) -> Operand {
    let out = get(ctx, recv, index, key);
    let Some(flag) = borrowed else {
        return out;
    };
    let owned = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Bool, Operand::Value(flag), 0),
        Type::Bool,
        None,
    );
    let drop_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cond = crate::ssa::Terminator::CondBr {
        cond: Operand::Value(owned),
        then_blk: drop_blk,
        else_blk: after,
    };
    ctx.f.set_term(ctx.cur_block, cond);
    ctx.cur_block = drop_blk;
    ctx.emit_drop_value(Operand::Value(key), Type::Str);
    ctx.f
        .set_term(ctx.cur_block, crate::ssa::Terminator::Br(after));
    ctx.cur_block = after;
    out
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
/// does, and an `any` local takes the slot back afterwards. `entry` is
/// the slot the read on the right-hand side answered its entry in
/// ([`super::dict_entry`]); the write is then the same one call with
/// that slot added.
pub(crate) fn set(
    ctx: &mut LowerCtx<'_>,
    recv: Operand,
    key: ValueId,
    boxed: Operand,
    recv_ident: &Option<String>,
    entry: Option<ValueId>,
) {
    let slot = ctx.alloca(Type::Any, Some("__any_recv_slot"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(recv, Operand::Value(slot), 0),
    );
    let strict = !crate::ssa_lower_set_flavor::cur_fn_is_sloppy(ctx);
    let mut args = vec![
        Operand::Value(slot),
        Operand::Value(key),
        boxed,
        Operand::ConstBool(strict),
    ];
    let callee = match entry {
        Some(e) => {
            args.push(Operand::Value(e));
            ctx.fn_table[SET_ENTRY]
        }
        None => ctx.fn_table[SET],
    };
    ctx.f
        .append_void(ctx.cur_block, InstKind::Call(callee, args));
    ctx.emit_throw_check(None);
    crate::ssa_lower_assign_member_any::write_back_recv(ctx, slot, recv_ident);
}
