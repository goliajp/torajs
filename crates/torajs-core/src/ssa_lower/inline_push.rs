//! `xs.push(<instance>)` onto an array that holds its class in place
//! (`Type::Inline`): the instance is built in the slot past the end,
//! field by field, with no cell and no factory call.
//!
//! The argument runs first and in full (it may push to the same array
//! itself), and only then is the slot found: a lock bit (frozen,
//! sealed, not extensible, read-only `length`) raises the TypeError a
//! push raises; a buffer with no room for the element grows on a cold
//! edge; each field is stored at `data + len * stride + 8k` and the
//! length moves up by one. The grow asks for the slots in use plus the
//! slots needed, at least twice what the buffer holds, since the
//! runtime grows by exactly what it is asked.
//!
//! The pre-reserved fill loops (`crate::ssa_lower_arr_prereserve`)
//! reserve `fields` slots per element for such an array and append
//! through [`push_prereserved`]; a construction from arguments that
//! cannot call is no call (`construction_args`), so those loops take
//! the inline array as they take a scalar one.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, IPred, InstKind, Operand, StructId, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_LEN_OFF, LowerCtx, PreReserveState};
use crate::ssa_lower_checked_cast::{Admits, CastScope, CastSlot};
use crate::ssa_lower_pass_0_5::value_class_census::FieldInit;

use super::inline_elem::{elem_addr, elem_stride, emit_elem_addr, emit_scaled_index};

/// The header bits that refuse an append (§10.1.8.1) or the length
/// write that follows it.
const APPEND_LOCK_BITS: u16 = torajs_rc::FLAG_FROZEN
    | torajs_rc::FLAG_SEALED
    | torajs_rc::FLAG_NON_EXTENSIBLE
    | torajs_rc::FLAG_ARR_LENGTH_RO;

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
    // raises the TypeError the runtime push raises, reading only the
    // array's header
    crate::ssa_lower_str_arr_inplace::emit_arr_lock_bits_guard(
        ctx,
        arr,
        APPEND_LOCK_BITS,
        None,
        None,
    );
    let k = fields.len() as u64;
    let load = |ctx: &mut LowerCtx<'_>, off: u64| {
        let cb = ctx.cur_block;
        let v = ctx
            .f
            .append_inst(cb, InstKind::Load(Type::I64, arr, off), Type::I64, None);
        Operand::Value(v)
    };
    let packed = load(ctx, ARR_CAP_HEAD_OFF);
    let cap = ctx.bin(
        BinOp::And,
        packed,
        Operand::ConstI64(0xffff_ffff),
        Type::I64,
    );
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
    let target = ctx.bin(BinOp::Add, need, cap, Type::I64);
    let arr_ty = ctx.operand_ty(&arr);
    let reserve = InstKind::Call(ctx.intrinsics.arr_reserve, vec![arr, target]);
    ctx.f.append_inst(grow_blk, reserve, arr_ty, None);
    ctx.f.set_term(grow_blk, Terminator::Br(fast_blk));
    ctx.cur_block = fast_blk;
    let addr = elem_addr(ctx, arr, len, sid, true);
    store_fields(ctx, addr, &fields);
    let cb = ctx.cur_block;
    ctx.f
        .append_void(cb, InstKind::Store(len_next, arr, ARR_LEN_OFF));
    len_next
}

/// The append of a pre-reserved fill loop: the slot at the running
/// length is known to fit. Answers the new length.
pub(crate) fn push_prereserved(
    ctx: &mut LowerCtx<'_>,
    st: PreReserveState,
    arg: ExprId,
    sid: StructId,
) -> ValueId {
    let fields = field_values(ctx, arg, sid);
    let len_now = ctx.emit_prereserved_len(st);
    let stride = elem_stride(ctx.struct_layouts, Type::Inline(sid));
    let cb = ctx.cur_block;
    let off = emit_scaled_index(ctx.f, cb, Operand::Value(len_now), stride);
    let slot0 = Operand::Value(st.slot0);
    let addr = emit_elem_addr(ctx.f, cb, slot0, Operand::Value(off), sid);
    store_fields(ctx, addr, &fields);
    ctx.emit_prereserved_bump(st, len_now)
}

fn store_fields(ctx: &mut LowerCtx<'_>, addr: ValueId, fields: &[Operand]) {
    let cb = ctx.cur_block;
    for (k, v) in fields.iter().enumerate() {
        let store = InstKind::Store(*v, Operand::Value(addr), 8 * k as u64);
        ctx.f.append_void(cb, store);
    }
}

/// The fields of the instance `arg` builds, in layout order, each in
/// its slot's type. The construction's arguments, or the literal's
/// members, run in source order before any field is stored.
fn field_values(ctx: &mut LowerCtx<'_>, arg: ExprId, sid: StructId) -> Vec<Operand> {
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let by_name: Vec<(String, Operand)> = if let Some(args) = construction_args(ctx, arg) {
        let argv = factory_args(ctx, arg, args);
        let inits = ctx.proofs.value_inits(sid).to_vec();
        assert_eq!(
            inits.len(),
            layout.len(),
            "ssa-lower: inits of sid {}",
            sid.0
        );
        (inits.into_iter())
            .map(|(f, init)| {
                let v = match init {
                    FieldInit::Arg(i) => argv[i],
                    FieldInit::Number(n) => Operand::ConstF64(n),
                    FieldInit::Bool(b) => Operand::ConstBool(b),
                };
                (f, v)
            })
            .collect()
    } else if let Expr::ObjectLit { fields } = ctx.ast.get_expr(arg) {
        (fields.iter())
            .map(|(k, v)| (k.lossy().into_owned(), ctx.lower_expr(*v)))
            .collect()
    } else {
        panic!(
            "internal error: an inline element built from {:?}",
            ctx.ast.get_expr(arg)
        )
    };
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
