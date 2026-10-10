//! The declared parameters of a this-first / factory callee, taken
//! off the `Array<Any>` a spread call materialized (§10.2.11: slot
//! `i` is parameter `i`, past the end it is undefined).
//!
//! Each slot crosses the way an argument entering the same body
//! through its boxed argv adapter does (`ssa_lower_boxed_entry`):
//! a scalar / any / string slot converts like a positional argument;
//! a parameter annotated with a class or an array of one takes the
//! checked cast; any other heap-typed slot takes the box's cell as a
//! borrow, which the list keeps alive across the call.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::CastSlot;

/// One declared parameter: its SSA type and its annotation.
pub(super) type Declared = Vec<(Type, Option<String>)>;

/// What the unpack leaves for after the call: the casts' and
/// conversions' own values to release, and the borrowed cells whose
/// boxes may have minted a materialization to settle.
#[derive(Default)]
pub(super) struct Unpacked {
    pub(super) owned: Vec<(Operand, Type)>,
    pub(super) settle: Vec<(ValueId, ValueId)>,
}

/// The declared parameters of `name` past `skip` signature slots
/// (the head, the hidden count, the argv pointer), when every one of
/// them can be unpacked; `None` before anything is emitted otherwise.
/// A rest parameter collects the tail rather than one slot, which
/// this lane does not build.
pub(super) fn declared(
    ctx: &LowerCtx<'_>,
    name: &str,
    head: usize,
    skip: usize,
) -> Option<Declared> {
    let target = *ctx.fn_table.get(name)?;
    let sig_id = *ctx.fn_sig_ids.get(&target)?;
    let tys = ctx.fn_sigs[sig_id.0 as usize].0.get(skip..)?.to_vec();
    let user: Vec<_> = ctx
        .ast
        .stmts
        .iter()
        .find_map(|s| match s {
            Stmt::FnDecl {
                name: n, params, ..
            } if n == name => Some(params),
            _ => None,
        })?
        .iter()
        .skip(head)
        .filter(|p| p.name != "__torajs_argv")
        .collect();
    if user.iter().any(|p| p.is_rest) {
        return None;
    }
    if !tys
        .iter()
        .all(|t| converts(*t) || (t.is_refcounted() && *t != Type::Substr))
    {
        return None;
    }
    let anns = (user.len() == tys.len()).then(|| user.iter().map(|p| p.type_ann.clone()));
    let anns: Vec<Option<String>> = match anns {
        Some(a) => a.collect(),
        None => vec![None; tys.len()],
    };
    Some(tys.into_iter().zip(anns).collect())
}

/// Emit the unpack of `declared` off `args_arr`; answers the call
/// operands. `spread_eid` stands in for the argument expression the
/// value has none of.
pub(super) fn emit(
    ctx: &mut LowerCtx<'_>,
    args_arr: Operand,
    spread_eid: ExprId,
    declared: Declared,
    out: &mut Unpacked,
) -> Vec<Operand> {
    let mut ops = Vec::with_capacity(declared.len());
    // a cast below may throw, with the list and the earlier casts live
    let mut parked = vec![ctx.push_throw_temp(args_arr, ctx.operand_ty(&args_arr))];
    for (i, (pty, ann)) in declared.into_iter().enumerate() {
        let slot = crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            args_arr,
            Operand::ConstI64(i as i64),
        );
        let cast = match (pty, ann.as_deref()) {
            (Type::Obj(sid), Some(a)) => {
                ctx.checked_cast_obj(Operand::Value(slot), sid, a, CastSlot::Pass)
            }
            (Type::Arr(_), Some(a)) => {
                ctx.checked_cast_obj_arr(Operand::Value(slot), pty, a, CastSlot::Pass)
            }
            _ => None,
        };
        let v = match cast {
            Some(v) => {
                out.owned.push((v, pty));
                parked.push(ctx.push_throw_temp(v, pty));
                v
            }
            None if !converts(pty) => borrow_cell(ctx, slot, pty, out),
            None => {
                let before = out.owned.len();
                let v = crate::ssa_lower_call_arg_conv::emit_arg_conv(
                    ctx,
                    pty,
                    spread_eid,
                    Operand::Value(slot),
                    &mut out.owned,
                );
                for &(op, ty) in &out.owned[before..] {
                    parked.push(ctx.push_throw_temp(op, ty));
                }
                v
            }
        };
        ops.push(v);
    }
    for t in parked.into_iter().rev() {
        ctx.pop_throw_temp(t);
    }
    ops
}

/// The slot types a positional argument's conversion takes out of a
/// box (`emit_arg_conv`).
fn converts(t: Type) -> bool {
    matches!(
        t,
        Type::Any | Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Str | Type::BigInt
    )
}

/// The cell a heap-typed slot's box holds, borrowed.
fn borrow_cell(ctx: &mut LowerCtx<'_>, slot: ValueId, pty: Type, out: &mut Unpacked) -> Operand {
    let raw = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![Operand::Value(slot)]),
        Type::I64,
        None,
    );
    out.settle.push((slot, raw));
    Operand::Value(ctx.f.append_inst(
        ctx.cur_block,
        InstKind::IntToPtr(Operand::Value(raw)),
        pty,
        None,
    ))
}

/// After the call: release what the unpack owns, and settle the
/// borrowed cells.
pub(super) fn release(ctx: &mut LowerCtx<'_>, out: Unpacked) {
    for (op, ty) in out.owned {
        ctx.emit_drop_value(op, ty);
    }
    for (av, raw) in out.settle {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.any_unbox_settle,
                vec![Operand::Value(av), Operand::Value(raw)],
            ),
        );
    }
}
