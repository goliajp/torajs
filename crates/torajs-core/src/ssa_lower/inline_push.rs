//! `xs.push(<instance>)` onto an array that holds its class in place
//! (`Type::Inline`): the instance is built in the slot past the end,
//! field by field, with no cell and no factory call.
//!
//! The argument runs first and in full (it may push to the same array
//! itself), and only then is the slot found: a buffer with no room for
//! the element grows on a cold edge; each field is stored at
//! `data + len * stride + 8k` and the length moves up by one. The grow
//! asks for the slots in use plus the slots needed, at least twice what
//! the buffer holds, since the runtime grows by exactly what it is
//! asked, and never past the u32 slot count a capacity can hold; an
//! append that needs more raises the RangeError of §10.4.2.4.
//!
//! There is no test of the header's lock bits (frozen, sealed, not
//! extensible, read-only `length`), which refuse a push with a
//! TypeError (§10.1.8.1): the census admits a class only when its
//! arrays reach nothing but the places it lists, and none of them can
//! set a lock bit, so no array that holds its elements in place has
//! one. A debug compiler, and the strict switch the conformance gate
//! sets, emit a trap on a lock bit instead ([`assert_unlocked`]).
//!
//! The pre-reserved fill loops (`crate::ssa_lower_arr_prereserve`)
//! reserve `fields` slots per element for such an array and append
//! through [`push_prereserved`]; a construction from arguments that
//! cannot call is no call (`construction_args`), so those loops take
//! the inline array as they take a scalar one.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, StructId, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_LEN_OFF, LowerCtx, PreReserveState};
use crate::ssa_lower_checked_cast::{Admits, CastScope, CastSlot};
use crate::ssa_lower_pass_0_5::value_class_census::FieldInit;

use super::inline_elem::{elem_addr, elem_stride, emit_elem_addr, emit_scaled_index};

/// The arguments of `eid` when it builds an instance of a class whose
/// arrays hold it in place (`new C(..)`, or the `__new_C(..)` call the
/// class desugaring spells it as).
pub(crate) fn construction_args<'e>(ctx: &LowerCtx<'e>, eid: ExprId) -> Option<&'e [ExprId]> {
    if !ctx.arr_layouts.has_value_classes() {
        return None;
    }
    let ast = ctx.ast;
    let (class, args) = match ast.get_expr(eid) {
        Expr::Call { callee, args } => match ast.get_expr(*callee) {
            Expr::Ident(n) => (n.strip_prefix("__new_")?, args),
            _ => return None,
        },
        Expr::New {
            class_name, args, ..
        } => (class_name.as_str(), args),
        _ => return None,
    };
    match ctx.aliases.get(class) {
        Some(Type::Obj(sid)) if ctx.arr_layouts.holds_in_place(*sid) => Some(args),
        _ => None,
    }
}

/// `arr.push(arg)`; answers the new length.
pub(crate) fn push(ctx: &mut LowerCtx<'_>, arr: Operand, arg: ExprId, sid: StructId) -> Operand {
    let fields = field_values(ctx, arg, sid);
    if cfg!(debug_assertions) || crate::ssa_lower_pass_2_5::value_class_check::strict() {
        assert_unlocked(ctx, arr);
    }
    let k = fields.len() as u64;
    let load = |ctx: &mut LowerCtx<'_>, off: u64| {
        let cb = ctx.cur_block;
        let v = ctx
            .f
            .append_inst(cb, InstKind::Load(Type::I64, arr, off), Type::I64, None);
        Operand::Value(v)
    };
    let cap = load_cap(ctx, arr);
    let len = load(ctx, ARR_LEN_OFF);
    let len_next = ctx.bin(BinOp::Add, len, Operand::ConstI64(1), Type::I64);
    let cb = ctx.cur_block;
    let need = Operand::Value(emit_scaled_index(ctx.f, cb, len_next, k));
    let full = ctx.cmp(IPred::Ult, cap, need);
    let grow_blk = ctx.f.add_block();
    let fast_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: full,
            then_blk: grow_blk,
            else_blk: fast_blk,
        },
    );
    ctx.cur_block = grow_blk;
    emit_grow_to_fit(ctx, arr, need, cap, k, &mut None);
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(fast_blk));
    ctx.cur_block = fast_blk;
    let addr = elem_addr(ctx, arr, len, sid, true);
    store_fields(ctx, addr, &fields);
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Store(len_next, arr, ARR_LEN_OFF));
    len_next
}

/// The capacity of `arr`'s buffer, in slots.
fn load_cap(ctx: &mut LowerCtx<'_>, arr: Operand) -> Operand {
    let cb = ctx.cur_block;
    let packed = InstKind::Load(Type::I64, arr, ARR_CAP_HEAD_OFF);
    let packed = Operand::Value(ctx.f.append_inst(cb, packed, Type::I64, None));
    // the low half of the packed word; the deque head is the high half
    ctx.bin(
        BinOp::And,
        packed,
        Operand::ConstI64(0xffff_ffff),
        Type::I64,
    )
}

/// Grow `arr`, whose buffer holds `cap` slots of `k`-slot elements, to
/// at least `need` slots: twice what it holds, since the runtime grows
/// by exactly what it is asked, and never past the u32 slot count a
/// capacity can hold; a `need` past that raises the RangeError of
/// §10.4.2.4, from the block in `raise` (`emit_too_long_guard`).
pub(crate) fn emit_grow_to_fit(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    need: Operand,
    cap: Operand,
    k: u64,
    raise: &mut Option<BlockId>,
) {
    // the capacity is a u32 count of slots, so an array of `k`-slot
    // elements holds at most `u32::MAX / k` of them
    let max = (u32::MAX as u64 / k * k) as i64;
    let over = ctx.cmp(IPred::Sgt, need, Operand::ConstI64(max));
    ctx.emit_too_long_guard(over, raise);
    let doubled = ctx.bin(BinOp::Add, need, cap, Type::I64);
    // min(doubled, max) by arithmetic: the lowering may not form a
    // select, which only the passes after the egraph introduce
    let room = ctx.bin(BinOp::Sub, Operand::ConstI64(max), doubled, Type::I64);
    let sign = ctx.bin(BinOp::AShr, room, Operand::ConstI64(63), Type::I64);
    let short = ctx.bin(BinOp::And, room, sign, Type::I64);
    let target = ctx.bin(BinOp::Add, doubled, short, Type::I64);
    let cb = ctx.cur_block;
    let arr_ty = ctx.operand_ty(&arr);
    let reserve = InstKind::Call(ctx.intrinsics.arr_reserve, vec![arr, target]);
    ctx.f.append_inst(cb, reserve, arr_ty, None);
}

/// The header bits that refuse an append (§10.1.8.1) or the length
/// write that follows it.
const APPEND_LOCK_BITS: u16 = torajs_rc::FLAG_FROZEN
    | torajs_rc::FLAG_SEALED
    | torajs_rc::FLAG_NON_EXTENSIBLE
    | torajs_rc::FLAG_ARR_LENGTH_RO;

/// Traps when `arr` carries a lock bit, which the census promises no
/// array holding its elements in place can. Emitted only by a debug
/// compiler or under the value-class strict switch (the conformance
/// gate), so a shipped program pays nothing.
fn assert_unlocked(ctx: &mut LowerCtx<'_>, arr: Operand) {
    let cb = ctx.cur_block;
    let word = ctx
        .f
        .append_inst(cb, InstKind::Load(Type::I32, arr, 4), Type::I32, None);
    let mask = Operand::ConstI32(((APPEND_LOCK_BITS as u32) << 16) as i32);
    let bits = ctx.bin(BinOp::And, Operand::Value(word), mask, Type::I32);
    let locked = ctx.cmp(IPred::Ne, bits, Operand::ConstI32(0));
    let trap_blk = ctx.f.add_block();
    let cont_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: locked,
            then_blk: trap_blk,
            else_blk: cont_blk,
        },
    );
    ctx.f.set_term(trap_blk, Terminator::Unreachable);
    ctx.cur_block = cont_blk;
}

/// The append of a pre-reserved fill loop: the slot at the running
/// length is known to fit, or, on a guarded loop, is made to fit on a
/// cold edge first (`ssa_lower_arr_prereserve::guard`). Answers the
/// new length.
pub(crate) fn push_prereserved(
    ctx: &mut LowerCtx<'_>,
    st: PreReserveState,
    arg: ExprId,
    sid: StructId,
) -> ValueId {
    let fields = field_values(ctx, arg, sid);
    let stride = elem_stride(ctx.struct_layouts, Type::Inline(sid));
    let (len_now, slot0) = match (st.guard, st.slot0) {
        (None, Some(slot0)) => (ctx.emit_prereserved_len(st), Operand::Value(slot0)),
        (Some(g), _) => {
            let (len, fast_blk) = ctx.emit_room_test(st, g);
            make_room(ctx, st, len, stride / 8, fast_blk);
            ctx.cur_block = fast_blk;
            (len, ctx.emit_slot0(st.arr_ptr, g))
        }
        (None, None) => unreachable!("an unguarded fill loop holds slot[0]"),
    };
    let cb = ctx.cur_block;
    let off = emit_scaled_index(ctx.f, cb, Operand::Value(len_now), stride);
    let addr = emit_elem_addr(ctx.f, cb, slot0, Operand::Value(off), sid);
    store_fields(ctx, addr, &fields);
    ctx.emit_prereserved_bump(st, len_now)
}

/// The cold edge of a guarded append of a `k`-slot element at `len`:
/// grow the buffer when the slot is not there, then go on to
/// `fast_blk`. The budget may be behind a grow made earlier in the
/// loop, so the buffer's own capacity decides.
fn make_room(ctx: &mut LowerCtx<'_>, st: PreReserveState, len: ValueId, k: u64, fast_blk: BlockId) {
    let arr = Operand::Value(st.arr_ptr);
    if cfg!(debug_assertions) || crate::ssa_lower_pass_2_5::value_class_check::strict() {
        assert_unlocked(ctx, arr);
    }
    // no head: the census keeps a class whose arrays' head may move
    // from being held in place
    let cap = load_cap(ctx, arr);
    let next = ctx.bin(
        BinOp::Add,
        Operand::Value(len),
        Operand::ConstI64(1),
        Type::I64,
    );
    let cb = ctx.cur_block;
    let need = Operand::Value(emit_scaled_index(ctx.f, cb, next, k));
    let full = ctx.cmp(IPred::Ult, cap, need);
    let grow_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: full,
            then_blk: grow_blk,
            else_blk: fast_blk,
        },
    );
    ctx.cur_block = grow_blk;
    // the grow may raise
    ctx.emit_settle_len(st, len);
    let mut raise = ctx.prereserve.walk_raise.take();
    emit_grow_to_fit(ctx, arr, need, cap, k, &mut raise);
    ctx.prereserve.walk_raise = raise;
    let cb = ctx.cur_block;
    ctx.f.set_term(cb, Terminator::Br(fast_blk));
}

fn store_fields(ctx: &mut LowerCtx<'_>, addr: ValueId, fields: &[Operand]) {
    let cb = ctx.cur_block;
    for (k, v) in fields.iter().enumerate() {
        let store = InstKind::Store(*v, Operand::Value(addr), 8 * k as u64);
        ctx.f.append_void(cb, store);
    }
}

/// The fields of the instance `arg` builds, in layout order, each in
/// its slot's type. The construction's arguments run in source order
/// before any field is stored. The census admits no other argument (an
/// object literal is a struct to the checker, not an instance), and
/// checked that each field's slot takes what the construction puts
/// there.
fn field_values(ctx: &mut LowerCtx<'_>, arg: ExprId, sid: StructId) -> Vec<Operand> {
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let Some(args) = construction_args(ctx, arg) else {
        panic!(
            "internal error: an inline element built from {:?}",
            ctx.ast.get_expr(arg)
        )
    };
    let argv = factory_args(ctx, arg, args);
    let inits = ctx.proofs.value_inits(sid).to_vec();
    assert_eq!(
        inits.len(),
        layout.len(),
        "ssa-lower: inits of sid {}",
        sid.0
    );
    let by_name: Vec<(String, Operand)> = (inits.into_iter())
        .map(|(f, init)| {
            let v = match init {
                FieldInit::Arg(i) => argv[i],
                FieldInit::Number(n) => Operand::ConstF64(n),
                FieldInit::Bool(b) => Operand::ConstBool(b),
            };
            (f, v)
        })
        .collect();
    (layout.iter())
        .map(|(k, ty)| {
            let (_, v) = (by_name.iter())
                .find(|(n, _)| k == n.as_str())
                .unwrap_or_else(|| panic!("internal error: no value for inline field {k:?}"));
            fit(ctx, *v, *ty)
        })
        .collect()
}

/// The arguments of a construction, each converted to the factory's
/// parameter as the call would convert it.
fn factory_args(ctx: &mut LowerCtx<'_>, eid: ExprId, args: &[ExprId]) -> Vec<Operand> {
    let class = match ctx.ast.get_expr(eid) {
        Expr::New { class_name, .. } => class_name.clone(),
        Expr::Call { callee, .. } => match ctx.ast.get_expr(*callee) {
            Expr::Ident(n) => n.trim_start_matches("__new_").to_string(),
            _ => unreachable!("a construction's callee is its factory"),
        },
        _ => unreachable!("a construction is a call or a `new`"),
    };
    let fid = ctx.fn_table[&format!("__new_{class}")];
    let params = (ctx.fn_sig_ids.get(&fid))
        .map(|s| ctx.fn_sigs[s.0 as usize].0.clone())
        .unwrap_or_default();
    // the census admits a factory whose parameters are the user's alone
    assert_eq!(
        params.len(),
        args.len(),
        "ssa-lower: the factory of `{class}`"
    );
    let mut owned = Vec::new();
    let argv: Vec<Operand> = (args.iter().enumerate())
        .map(|(i, a)| {
            let v = ctx.lower_expr(*a);
            crate::ssa_lower_call_arg_conv::emit_arg_conv(ctx, params[i], *a, v, &mut owned)
        })
        .collect();
    // a number or boolean conversion mints nothing to release
    assert!(
        owned.is_empty(),
        "ssa-lower: an inline construction minted an argument"
    );
    argv
}

/// `v` in a field of type `ty`: an `any` unboxes as a field store would
/// unbox it, an integer widens into a float slot, and a literal takes
/// the slot's width.
fn fit(ctx: &mut LowerCtx<'_>, v: Operand, ty: Type) -> Operand {
    if let Some(cast) = ctx.checked_cast(v, ty, CastSlot::Store, CastScope::Inferred, Admits::Exact)
    {
        return cast;
    }
    match (ty, v) {
        (Type::I64, Operand::ConstF64(n)) if n.fract() == 0.0 && n.abs() < 9.007e15 => {
            return Operand::ConstI64(n as i64);
        }
        (Type::Bool, Operand::ConstBool(_)) | (Type::F64, Operand::ConstF64(_)) => return v,
        _ => {}
    }
    match (ty, ctx.operand_ty(&v)) {
        (Type::F64, Type::I64) => ctx.coerce_to_f64(v),
        (want, have) if want == have => v,
        (want, have) => panic!("internal error: a {have:?} value for an inline {want:?} field"),
    }
}
