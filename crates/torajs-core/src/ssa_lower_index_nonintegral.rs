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
/// whose arrays never grow one links no any world here. A boolean or
/// pointer element asks the `_any` twin ([`emit_any_shaped_read`]).
/// Answers `false` for an element type with no undefined spelling
/// (an integer): the caller keeps its RangeError exit. Leaves
/// `cur_block` on the block that stored the slot, with no terminator.
///
/// The out-of-range exit asks the same entries, with the integer
/// index as the key, when the program can put a property on an
/// array's prototype chain (`lower_typed_index_checked`).
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
        Type::Bool => return emit_any_shaped_read(ctx, arr, raw_key, elem_ty, slot),
        t if t.spells_undef_with_generic_cell() => {
            return emit_any_shaped_read(ctx, arr, raw_key, elem_ty, slot);
        }
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
    check_keyed_read(ctx);
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

/// The throw check after a typed lane's keyed read. A getter on the
/// property may throw, as may the `valueOf` the `number[]` lane's
/// ToNumber runs, and neither answer may reach the slot. Where no
/// such property can run code (`ProgramFacts::nonint_reads_run_code`
/// false) the read cannot raise, and the throw analysis told every
/// caller of this fn so: a check here would only lead to a throw
/// path its callers never test. The seam's reject stub, which
/// records a TypeError when the link judgment missed a writer, then
/// surfaces at the next check instead of at this read.
fn check_keyed_read(ctx: &mut LowerCtx<'_>) {
    if ctx.proofs.nonint_reads_run_code() {
        ctx.emit_throw_check(None);
    }
}

/// The out-of-range exit of a typed read in a program that can put a
/// property on an array's prototype chain: `xs[5]` past the end, or
/// `xs[-1]`, is no own element, and §10.1.8.1 OrdinaryGet goes on to
/// the property the key names on the chain — through the same entries
/// a non-integral key asks, with the index as the key. Builds the
/// block the bounds branch takes (it stores the slot and goes to
/// `merge`) and leaves `cur_block` alone; `None` for an element with
/// no undefined spelling, whose exit stays the RangeError.
pub(crate) fn emit_oob_named_read(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    idx: Operand,
    elem_ty: Type,
    slot: ValueId,
    merge: crate::ssa::BlockId,
) -> Option<crate::ssa::BlockId> {
    if !matches!(elem_ty, Type::F64 | Type::Str | Type::Bool)
        && !elem_ty.spells_undef_with_generic_cell()
    {
        return None;
    }
    let saved = ctx.cur_block;
    let ask = ctx.f.add_block();
    ctx.cur_block = ask;
    let key = ctx
        .f
        .append_inst(ask, InstKind::SiToFp(idx), Type::F64, None);
    emit_nonintegral_typed_read(ctx, arr, Operand::Value(key), elem_ty, slot);
    let end = ctx.cur_block;
    ctx.f.set_term(end, crate::ssa::Terminator::Br(merge));
    ctx.cur_block = saved;
    Some(ask)
}

/// The index a string's view read takes: §10.4.3.5 answers an own
/// property only for a CanonicalNumericIndexString that is an integer,
/// so a key that is not one (`s[1.5]`, `s[NaN]`) reads as -1, which
/// the view answers with its `undefined` like any index past the end.
/// Used where no program code can put a property on
/// %String.prototype% (`Ast::array_prop_reads_may_raise` false), so
/// that `undefined` is the property's value.
pub(crate) fn str_view_index(
    ctx: &mut LowerCtx<'_>,
    idx: Operand,
    nonint: Option<(Operand, Operand)>,
) -> Operand {
    let Some((flag, _)) = nonint else {
        return idx;
    };
    // idx | -flag: all ones when the flag is set (no Select before
    // the egraph pass forms them)
    use crate::ssa::BinOp;
    let cb = ctx.cur_block;
    let one = ctx
        .f
        .append_inst(cb, InstKind::ZExtBoolToI64(flag), Type::I64, None);
    let mask = ctx.f.append_inst(
        cb,
        InstKind::BinOp(BinOp::Sub, Operand::ConstI64(0), Operand::Value(one)),
        Type::I64,
        None,
    );
    let v = ctx.f.append_inst(
        cb,
        InstKind::BinOp(BinOp::Or, idx, Operand::Value(mask)),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

/// A string's index read in a program that can put a property on
/// %String.prototype% or %Object.prototype%: a key that is not an
/// in-range integer reads the property it names, through the keyed
/// kernel, which runs a getter and records its throw
/// (`__torajs_str_index_keyed`).
pub(crate) fn emit_str_keyed_read(
    ctx: &mut LowerCtx<'_>,
    s: Operand,
    idx: Operand,
    nonint: Option<(Operand, Operand)>,
) -> Operand {
    let key = match nonint {
        Some((_, raw)) => raw,
        None => {
            Operand::Value(
                ctx.f
                    .append_inst(ctx.cur_block, InstKind::SiToFp(idx), Type::F64, None),
            )
        }
    };
    let fid = *ctx
        .fn_table
        .get("__torajs_str_index_keyed")
        .expect("string keyed read entry missing");
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![s, key]),
        Type::Substr,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// [`emit_nonintegral_typed_read`]'s arm for a `boolean[]`, whose read
/// is promoted to an `any` slot and keeps the property's value whole,
/// and for a pointer element, whose slot has no spelling for a value
/// of another type: there the read runs for its getter's throw and
/// the slot takes `undefined`.
fn emit_any_shaped_read(
    ctx: &mut LowerCtx<'_>,
    arr: Operand,
    raw_key: Operand,
    elem_ty: Type,
    slot: ValueId,
) -> bool {
    let fid = *ctx
        .fn_table
        .get("__torajs_arr_typed_nonint_any")
        .expect("typed non-integral read entry missing");
    let keep = elem_ty == Type::Bool;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![arr, raw_key, Operand::ConstI64(keep as i64)]),
        Type::Any,
        None,
    );
    check_keyed_read(ctx);
    let stored = if keep {
        Operand::Value(v)
    } else {
        let c = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
            elem_ty,
            None,
        );
        Operand::Value(c)
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(stored, Operand::Value(slot), 0),
    );
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
