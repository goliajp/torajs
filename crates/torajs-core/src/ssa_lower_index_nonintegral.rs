//! A Number index that is not an integer, on the TYPED array lanes —
//! `xs[1.5]` names the property "1.5", never element 1 (§7.1.19).
//! The read carries a "not an integer" flag next to the truncated
//! index and, when it fires, asks torajs-arr for the property (the
//! slot's undefined unless the cell can carry one); the store's arm
//! lives in `ssa_lower_index_assign_guards`. Sibling of
//! [`crate::ssa_lower_index_any_key`], whose `AnyNumericKey` is the
//! same split for an `any` receiver.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_index_any_key::AnyNumericKey;

/// The typed read's index, plus — for an index the SSA types F64 —
/// the "not an integer" flag and the raw f64. §7.1.19: `xs[1.5]`
/// names the property "1.5", never element 1, and `xs[NaN]` names
/// "NaN"; a typed array holds neither, so the read answers
/// `undefined` through the same exit an out-of-range index takes.
/// Before this every F64 index was truncated (`xs[1.5]` read
/// `xs[1]`, `xs[-0.5]` read `xs[0]`, `xs[NaN]` read `xs[0]`). The
/// truncation stays for the integral case — the flag is one
/// `fcvtzs` / `scvtf` / `fcmp` on top of it, on F64 indices only; an
/// I64 counter pays nothing.
pub(crate) fn lower_typed_read_index(
    ctx: &mut LowerCtx<'_>,
    index: ExprId,
) -> (Operand, Option<(Operand, Operand)>) {
    let raw = match &ctx.pins.compound_key {
        Some((mid, mop)) if *mid == index => *mop,
        _ => ctx.lower_expr(index),
    };
    if !matches!(ctx.operand_ty(&raw), Type::F64) {
        return (ctx.coerce_to_i64(raw), None);
    }
    let idx = ctx.coerce_to_i64(raw);
    let back = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::SiToFp(idx), Type::F64, None);
    let nonint = ctx.fcmp(crate::ssa::FPred::Une, Operand::Value(back), raw);
    (idx, Some((nonint, raw)))
}

/// The typed read's arm for an index that is not an integer: the
/// property "1.5" may exist as an expando (`xs[1.5] = v` makes one)
/// or on the prototype chain. `__torajs_arr_typed_nonint_f64` hands
/// back the F64 slot word (undefined as the sentinel, any other
/// value through ToNumber, the lane's recorded approximation), and
/// the `_str` twin an owned Str cell or NULL for the slot's
/// undefined cell. torajs-arr answers undefined itself for a cell
/// that cannot carry a named property; its keyed read sits behind a
/// link-judged seam (`cmd_build_elide`, fam 52 / 53), so a program
/// whose arrays never grow one links no any world here. Answers
/// `false` for an element type with no undefined spelling (i64 /
/// bool / nested heap): the caller keeps its RangeError exit. Leaves
/// `cur_block` on the block that stored the slot, with no terminator.
pub(crate) fn emit_nonintegral_typed_read(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    raw_key: Operand,
    elem_ty: Type,
    slot: ValueId,
) -> bool {
    use crate::ssa::{IPred, Terminator};
    let entry = match elem_ty {
        Type::F64 => "__torajs_arr_typed_nonint_f64",
        Type::Str => "__torajs_arr_typed_nonint_str",
        _ => return false,
    };
    let fid = *ctx
        .fn_table
        .get(entry)
        .expect("typed non-integral read entry missing");
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![arr, raw_key]),
        elem_ty,
        None,
    );
    // A getter on the property may throw, and the seam's reject
    // stub records one too; neither answer may reach the slot.
    ctx.emit_throw_check(None);
    if elem_ty == Type::F64 {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
        return true;
    }
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::PtrToInt(Operand::Value(v)),
        Type::I64,
        None,
    );
    let is_null = ctx.cmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(0));
    let undef_blk = ctx.f.add_block();
    let cell_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: is_null,
            then_blk: undef_blk,
            else_blk: cell_blk,
        },
    );
    let c = ctx.f.append_inst(
        undef_blk,
        InstKind::GlobalRef(crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM.to_string()),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        undef_blk,
        InstKind::Store(Operand::Value(c), Operand::Value(slot), 0),
    );
    ctx.f.set_term(undef_blk, Terminator::Br(done_blk));
    ctx.f.append_void(
        cell_blk,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    ctx.f.set_term(cell_blk, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    true
}

/// `Array<any>` read at an F64 index: an integer reads the element, a
/// non-integer asks the keyed kernel for the property it names — the
/// same split the typed lanes take, decided at run time by the flag
/// [`lower_typed_read_index`] computed. Before this the lane took the
/// keyed kernel for every F64 index, so `xs[i % 100]` boxed the array
/// and the key and dispatched generically on each read of a plain
/// element. Both arms answer an OWNED box (the kernel's answer is +1,
/// the element read takes its own share), which the caller records.
pub(crate) fn emit_any_elem_read_split(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    idx: Operand,
    (nonint, raw_key): (Operand, Operand),
) -> Operand {
    use crate::ssa::Terminator;
    let slot = ctx.alloca(Type::Any, None);
    let elem_blk = ctx.f.add_block();
    let key_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: nonint,
            then_blk: key_blk,
            else_blk: elem_blk,
        },
    );
    ctx.cur_block = elem_blk;
    // The read answers the whole (borrowed) box; the share this read
    // owns is the inc after it.
    let v = Operand::Value(crate::ssa_lower_anyv_inline::arr_get_any_elem(
        ctx, arr, idx,
    ));
    ctx.emit_any_rc_inc_in(ctx.cur_block, v);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(v, Operand::Value(slot), 0));
    let eb = ctx.cur_block;
    ctx.f.set_term(eb, Terminator::Br(done_blk));
    ctx.cur_block = key_blk;
    let boxed_arr = ctx.box_to_any(arr);
    let boxed_key = ctx.box_to_any(raw_key);
    let k = AnyNumericKey::Boxed(boxed_key).emit_get(ctx, boxed_arr);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(k), Operand::Value(slot), 0),
    );
    let kb = ctx.cur_block;
    ctx.f.set_term(kb, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    let out = ctx.f.append_inst(
        done_blk,
        InstKind::Load(Type::Any, Operand::Value(slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(out)
}
