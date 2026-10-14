//! The keyed read on a dictionary the checker typed with number values
//! (`Record<string, number>`).
//!
//! The read still answers `any` (ES §10.1.8.1 [[Get]]: the key may
//! name an inherited member, and an `any` alias may have stored a
//! value of any type), so the type only says which outcome to try
//! first. The read is one runtime call on NaN-box values
//! (`torajs-dynobj`'s `dict.rs`): an own entry holding a number is the
//! answer, and every other case runs inside the call exactly as the
//! generic pair read would. The call site is the call and its throw
//! check, no branches of its own.

use std::collections::HashMap;

use crate::ssa::{FuncId, InstKind, Module, Operand, Type, ValueId};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

/// `__torajs_dynobj_dict_get(recv, key) -> any`, an owned answer.
const GET: &str = "__torajs_dynobj_dict_get";

/// Declared in a program that has a number dictionary
/// (`Ast::number_dicts`) only; no other program calls it, so no other
/// program's SSA carries it.
pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    ast: &crate::ast::Ast,
) {
    if ast.number_dicts.is_empty() {
        return;
    }
    declare_intrinsic(module, fn_table, GET, &[Type::Any, Type::Ptr], Type::Any);
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
