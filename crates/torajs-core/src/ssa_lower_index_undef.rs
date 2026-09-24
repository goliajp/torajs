//! The `undefined` a typed element read answers where there is no
//! element — moved verbatim out of `ssa_lower_index.rs`
//! (`lower_typed_index_checked`'s out-of-range exit) when that file
//! reached its 500-line limit.

use crate::ssa::{BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
use crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS;

/// The value an out-of-range read of an `elem_ty` element stores into
/// its result slot, emitted into `oob_blk`, and whether that exit
/// records a RangeError instead (an integer element has no spelling
/// of `undefined`).
pub(crate) fn oob_undef_value(
    ctx: &mut LowerCtx<'_>,
    oob_blk: BlockId,
    elem_ty: Type,
) -> (Operand, bool) {
    let mut oob_throws = false;
    let oob_val: Operand = match elem_ty {
        Type::Str => {
            let sentinel = ctx.f.append_inst(
                oob_blk,
                InstKind::GlobalRef(STR_UNDEF_CELL_SYM.to_string()),
                Type::Str,
                None,
            );
            Operand::Value(sentinel)
        }
        Type::F64 => Operand::ConstF64(f64::from_bits(F64_UNDEF_SENTINEL_BITS)),
        // A pointer-shaped element spells `undefined` with the generic
        // immortal cell, which is how a `find` / `pop` miss on the same
        // array has answered since RFC 20260722 chunk B. Reading past
        // the end is the same question (§10.4.2.1), so it takes the
        // same exit rather than the RangeError below — `q.find(miss)`
        // answered `undefined` while `q[5]` on that very array threw.
        // Static cell: the result slot's scope drop no-ops.
        //
        // Emitted into `oob_blk` rather than through
        // `str_undef_sentinel_for`, which appends at `cur_block`: a
        // constant out-of-range index folds the first compare away and
        // takes that block with it, leaving the reference with no
        // definition (`nested[-1]` aborted the compile).
        t if t.spells_undef_with_generic_cell() => {
            let sentinel = ctx.f.append_inst(
                oob_blk,
                InstKind::GlobalRef(UNDEF_CELL_SYM.to_string()),
                elem_ty,
                None,
            );
            Operand::Value(sentinel)
        }
        // The promoted bool read spells `undefined` as the tag every
        // tagged value already uses for it.
        Type::Bool => {
            let undef = ctx.f.append_inst(
                oob_blk,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(undef)
        }
        _ => {
            oob_throws = true;
            ctx.f.append_void(
                oob_blk,
                InstKind::Call(ctx.intrinsics.arr_oob_throw, vec![]),
            );
            match elem_ty {
                Type::I64 | Type::I32 => Operand::ConstI64(0),
                _ => Operand::ConstPtrNull,
            }
        }
    };
    (oob_val, oob_throws)
}

/// A hole in a typed array whose elements are pointers (strings,
/// objects, arrays, functions …) is a NULL slot: the runtime fills a
/// grown gap with zero and marks it absent, and every any-lane reader
/// already answers `undefined` for it (`torajs-arr` `index_any.rs`).
/// The typed read handed the NULL on as an element — `s[4] = "z"` on
/// `["x", "y"]` made `s[3]` print `null` — so here it takes the same
/// `undefined` the out-of-range exit answers (§10.1.8.1: no own
/// element, and no prototype key a typed read would consult).
/// Scalar elements have no such slot value and emit nothing.
///
/// `v` is the element just loaded; on return `cur_block` continues
/// with it, the hole having stored into `slot` and gone to `merge`.
pub(crate) fn null_slot_is_hole(
    ctx: &mut LowerCtx<'_>,
    v: ValueId,
    elem_ty: Type,
    slot: ValueId,
    merge: BlockId,
) {
    if !(elem_ty == Type::Str || elem_ty.spells_undef_with_generic_cell()) {
        return;
    }
    let hole_blk = ctx.f.add_block();
    let elem_blk = ctx.f.add_block();
    let is_null = ctx.cmp(IPred::Eq, Operand::Value(v), Operand::ConstPtrNull);
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: is_null,
            then_blk: hole_blk,
            else_blk: elem_blk,
        },
    );
    let (undef, _) = oob_undef_value(ctx, hole_blk, elem_ty);
    ctx.f
        .append_void(hole_blk, InstKind::Store(undef, Operand::Value(slot), 0));
    ctx.f.set_term(hole_blk, Terminator::Br(merge));
    ctx.cur_block = elem_blk;
}

/// Does a typed element of `elem_ty` carry `undefined` as itself, to
/// wherever it goes next? A string or pointer does (an immortal
/// undefined cell). An `F64` spells it as a sentinel NaN only the
/// consumers that test for it understand — boxed or pushed, it is
/// NaN — and integer / boolean elements have no spelling.
pub(crate) fn has_undef_spelling(elem_ty: Type) -> bool {
    elem_ty == Type::Str || elem_ty.spells_undef_with_generic_cell()
}

/// `Get(O, k)` on a typed source whose length may have dropped below
/// `k` since the loop read it: `undefined` past the current length,
/// otherwise the element `read` loads. For the find family (§23.1.3.9
/// visits every index below the ORIGINAL length); the cut-off slots
/// are never loaded — on a pointer source the shrink released them.
pub(crate) fn emit_get_past_length(
    ctx: &mut LowerCtx<'_>,
    arr: ValueId,
    i: ValueId,
    elem_ty: Type,
    read: impl FnOnce(&mut LowerCtx<'_>) -> ValueId,
) -> ValueId {
    let slot = ctx.alloca_in_entry(elem_ty, Some("__get_elem"));
    let read_blk = ctx.f.add_block();
    let past_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let len_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(
            Type::I64,
            Operand::Value(arr),
            crate::ssa_lower::ARR_LEN_OFF,
        ),
        Type::I64,
        None,
    );
    let in_range = ctx.cmp(IPred::Slt, Operand::Value(i), Operand::Value(len_now));
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: in_range,
            then_blk: read_blk,
            else_blk: past_blk,
        },
    );
    let (undef, _) = oob_undef_value(ctx, past_blk, elem_ty);
    ctx.f
        .append_void(past_blk, InstKind::Store(undef, Operand::Value(slot), 0));
    ctx.f.set_term(past_blk, Terminator::Br(merge));
    ctx.cur_block = read_blk;
    let v = read(ctx);
    let cur = ctx.cur_block;
    ctx.f.append_void(
        cur,
        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
    );
    ctx.f.set_term(cur, Terminator::Br(merge));
    ctx.cur_block = merge;
    ctx.f.append_inst(
        merge,
        InstKind::Load(elem_ty, Operand::Value(slot), 0),
        elem_ty,
        None,
    )
}

/// The raw slot word a hole of a typed `elem_ty` cell holds — the
/// compiler-side twin of torajs-arr `grow_store::typed_hole_raw`. An
/// f64 element spells it with the undefined sentinel, which the typed
/// read above already answers as `undefined` (an index read of a
/// `number[]` is an undefined-f64 source) and which arithmetic turns
/// into the default NaN, ToNumber(undefined). Every other element type
/// keeps 0: a NULL pointer slot is read as a hole
/// ([`null_slot_is_hole`]); an integer or boolean slot has no word for
/// it yet.
pub(crate) fn typed_hole_raw(elem_ty: Type) -> Operand {
    if elem_ty == Type::F64 {
        Operand::ConstI64(F64_UNDEF_SENTINEL_BITS as i64)
    } else {
        Operand::ConstI64(0)
    }
}
