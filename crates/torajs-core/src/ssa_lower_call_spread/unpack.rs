//! The declared parameters of a this-first / factory callee, taken
//! off the `Array<Any>` a spread call materialized (§10.2.11: slot
//! `i` is parameter `i`, past the end it is undefined).
//!
//! Each slot crosses the way an argument entering the same body
//! through its boxed argv adapter does (`ssa_lower_boxed_entry`):
//! a scalar / any / string slot converts like a positional argument;
//! a parameter annotated with a class or an array of one takes the
//! checked cast; any other heap-typed slot takes the box's cell as a
//! borrow, which the list keeps alive across the call. A rest
//! parameter collects the slots past the fixed ones into a fresh
//! array, as the boxed adapter's rest slot does (`build_rest`).

use crate::ast::{ExprId, Stmt};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{LowerCtx, intern_arr_layout};
use crate::ssa_lower_checked_cast::CastSlot;

/// The declared parameters: each fixed one's SSA type and
/// annotation, then the rest parameter's slot when there is one.
pub(super) struct Declared {
    fixed: Vec<(Type, Option<String>)>,
    rest: Option<RestSlot>,
}

/// A rest parameter: its array type, and the element kind the
/// collected `Array<Any>` converts to (`None` for an `any` element).
#[derive(Clone, Copy)]
struct RestSlot {
    ty: Type,
    kind: Option<i64>,
}

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
pub(super) fn declared(
    ctx: &LowerCtx<'_>,
    name: &str,
    head: usize,
    skip: usize,
) -> Option<Declared> {
    let target = *ctx.fn_table.get(name)?;
    let sig_id = *ctx.fn_sig_ids.get(&target)?;
    let mut tys = ctx.fn_sigs[sig_id.0 as usize].0.get(skip..)?.to_vec();
    let mut user: Vec<_> = ctx
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
    let rest = match user.last() {
        Some(p) if p.is_rest => {
            user.pop();
            Some(rest_slot(ctx, tys.pop()?)?)
        }
        _ => None,
    };
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
    Some(Declared {
        fixed: tys.into_iter().zip(anns).collect(),
        rest,
    })
}

/// The rest slot of type `ty`, when its element is one the
/// `Array<Any>` conversion can stamp: a nested typed array's kind is
/// a chain the kernel takes one level of, as for the boxed adapter
/// (`rest_slot_kind`).
fn rest_slot(ctx: &LowerCtx<'_>, ty: Type) -> Option<RestSlot> {
    let Type::Arr(id) = ty else { return None };
    let kind = match ctx.arr_layouts[id.0 as usize] {
        Type::Any => None,
        Type::Arr(_) => return None,
        elem => Some(crate::ssa_lower_stmt_let_decl_convert::arr_elem_kind_const(
            &elem,
        )?),
    };
    Some(RestSlot { ty, kind })
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
    let n_fixed = declared.fixed.len();
    let mut ops = Vec::with_capacity(n_fixed + 1);
    // a cast below may throw, with the list and the earlier casts live
    let mut parked = vec![ctx.push_throw_temp(args_arr, ctx.operand_ty(&args_arr))];
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, args_arr, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    for (i, (pty, ann)) in declared.fixed.into_iter().enumerate() {
        let slot = crate::ssa_lower_anyv_inline::arr_get_any_borrowed(
            ctx,
            args_arr,
            Operand::ConstI64(i as i64),
        );
        let casts = match (pty, ann.as_deref()) {
            (Type::Obj(_), Some(a)) => ctx.casts_obj_slot(a),
            (Type::Arr(_), Some(a)) => ctx.casts_obj_arr_slot(pty, a),
            _ => false,
        };
        let cast = match ann.as_deref() {
            Some(a) if casts => Some(cast_if_passed(ctx, len, i, slot, pty, a)),
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
    if let Some(rest) = declared.rest {
        let v = collect_rest(ctx, args_arr, n_fixed, rest);
        out.owned.push((v, rest.ty));
        ops.push(v);
    }
    for t in parked.into_iter().rev() {
        ctx.pop_throw_temp(t);
    }
    ops
}

/// Slot `i` cast into the annotated parameter type `pty` when the
/// list holds it. Past the list's end no argument was passed, and
/// §10.2.11 binds the parameter to undefined, which no annotation
/// checks: the parameter takes its type's undefined cell. A position
/// the arity pads is not a passed argument, so it takes no cast.
fn cast_if_passed(
    ctx: &mut LowerCtx<'_>,
    len: ValueId,
    i: usize,
    slot: ValueId,
    pty: Type,
    ann: &str,
) -> Operand {
    let res = ctx.alloca_in_entry(pty, None);
    let passed = ctx.cmp(IPred::Slt, Operand::ConstI64(i as i64), Operand::Value(len));
    let (cast_blk, undef_blk, join) = (ctx.f.add_block(), ctx.f.add_block(), ctx.f.add_block());
    let b = ctx.cur_block;
    ctx.f.set_term(
        b,
        Terminator::CondBr {
            cond: passed,
            then_blk: cast_blk,
            else_blk: undef_blk,
        },
    );
    ctx.cur_block = cast_blk;
    let v = match pty {
        Type::Obj(sid) => ctx.checked_cast_obj(Operand::Value(slot), sid, ann, CastSlot::Pass),
        _ => ctx.checked_cast_obj_arr(Operand::Value(slot), pty, ann, CastSlot::Pass),
    }
    .expect("the slot was asked whether it casts");
    let b = ctx.cur_block;
    ctx.f
        .append_void(b, InstKind::Store(v, Operand::Value(res), 0));
    ctx.f.set_term(b, Terminator::Br(join));
    ctx.cur_block = undef_blk;
    let u =
        (ctx.str_undef_sentinel_for(pty)).expect("an object or array slot has an undefined cell");
    ctx.f
        .append_void(undef_blk, InstKind::Store(u, Operand::Value(res), 0));
    ctx.f.set_term(undef_blk, Terminator::Br(join));
    ctx.cur_block = join;
    Operand::Value(
        ctx.f
            .append_inst(join, InstKind::Load(pty, Operand::Value(res), 0), pty, None),
    )
}

/// The rest array: the list's slots past the `n_fixed` declared ones
/// (none when the list is shorter), in a fresh `Array<Any>`, then
/// converted to the typed array a typed rest declares. A slot the
/// element type does not admit raises the conversion's TypeError.
fn collect_rest(
    ctx: &mut LowerCtx<'_>,
    args_arr: Operand,
    n_fixed: usize,
    rest: RestSlot,
) -> Operand {
    let b = ctx.cur_block;
    let len = ctx.f.append_inst(
        b,
        InstKind::Load(Type::I64, args_arr, crate::ssa_lower::ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let n = ctx.f.append_inst(
        b,
        InstKind::BinOp(
            BinOp::Sub,
            Operand::Value(len),
            Operand::ConstI64(n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    // max(n, 0): the sign mask clears a negative count
    let sign = ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::AShr, Operand::Value(n), Operand::ConstI64(63)),
        Type::I64,
        None,
    );
    let keep = ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::Xor, Operand::Value(sign), Operand::ConstI64(-1)),
        Type::I64,
        None,
    );
    let count = Operand::Value(ctx.f.append_inst(
        b,
        InstKind::BinOp(BinOp::And, Operand::Value(n), Operand::Value(keep)),
        Type::I64,
        None,
    ));
    let any_ty = Type::Arr(intern_arr_layout(ctx.arr_layouts, Type::Any));
    let arr = Operand::Value(ctx.f.append_inst(
        b,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![count]),
        any_ty,
        None,
    ));
    let data = ctx.emit_arr_data_ptr(args_arr);
    let b = ctx.cur_block;
    let base = ctx
        .f
        .append_inst(b, InstKind::PtrToInt(data), Type::I64, None);
    let at = ctx.f.append_inst(
        b,
        InstKind::BinOp(
            BinOp::Add,
            Operand::Value(base),
            Operand::ConstI64(8 * n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let src = ctx
        .f
        .append_inst(b, InstKind::IntToPtr(Operand::Value(at)), Type::Ptr, None);
    // the slots are borrows of the list; the append takes stakes of
    // its own on the heap cells
    ctx.f.append_void(
        b,
        InstKind::Call(
            ctx.intrinsics.arr_any_push,
            vec![arr, Operand::Value(src), count, Operand::ConstPtrNull],
        ),
    );
    let Some(kind) = rest.kind else {
        return arr;
    };
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_any_to_typed")
        .expect("__torajs_arr_any_to_typed intrinsic missing");
    let typed = Operand::Value(ctx.f.append_inst(
        b,
        InstKind::Call(fid, vec![arr, Operand::ConstI64(kind)]),
        rest.ty,
        None,
    ));
    ctx.emit_drop_value(arr, any_ty);
    ctx.emit_throw_check(None);
    typed
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
