//! Composite-type arms (`Type::Arr` / `Type::Obj`) of
//! [`super::lower`], split out 2026-07-03 (fn-debt decomp). Bodies
//! verbatim from the pre-split match arms; element / field recursion
//! goes back through `super::lower`.

use crate::ssa::{ArrId, BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// §25.5.2 null gate shared by both composite arms — a NULL slot
/// (`Nullable<Arr>` exec/match miss, `Nullable<Obj>` holding JS
/// null) stringifies to `"null"`; the walk only runs on the non-null
/// branch (pre-fix the len / field loads dereferenced NULL, SIGSEGV;
/// same family as the 642 `json_quote_str` NULL arm on the Str
/// lane). The walked-result store reads `ctx.cur_block` AFTER the
/// walk, which may split blocks (656 lesson).
pub(super) fn with_null_gate(
    ctx: &mut LowerCtx,
    val_op: &Operand,
    slot_name: &str,
    walk: impl FnOnce(&mut LowerCtx) -> Operand,
) -> Operand {
    let out_slot = ctx.alloca_in_entry(Type::Str, Some(slot_name));
    let is_null_ptr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, *val_op, Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    // …and the generic immortal cell, which is a pointer slot's
    // in-band `undefined`. §25.5.2.4 step 8.a gives an array element
    // that serializes to nothing the text `null`, which is what this
    // gate already answers — the walk behind it would instead read
    // the cell as a live value of the slot's static type, which is
    // how `JSON.stringify([xs.find(p)])` on a miss became a SIGSEGV.
    // An object PROPERTY holding the cell never reaches here: that
    // lane omits the key first, through `__torajs_is_undef_cell`.
    let val_ty = ctx.operand_ty(val_op);
    let sentinel = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM.to_string()),
        val_ty,
        None,
    );
    let is_cell = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, *val_op, Operand::Value(sentinel)),
        Type::Bool,
        None,
    );
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::Or,
            Operand::Value(is_null_ptr),
            Operand::Value(is_cell),
        ),
        Type::Bool,
        None,
    );
    let null_blk = ctx.f.add_block();
    let walk_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: walk_blk,
        },
    );
    ctx.cur_block = null_blk;
    let null_str = ctx.intern_string_literal("null");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(null_str), Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = walk_blk;
    let walked = walk(ctx);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(walked, Operand::Value(out_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out_slot), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// §25.5.2.4 step 8.a — an element whose SerializeJSONProperty
/// answers nothing is `null` inside an array. The any-lane walk
/// reports that verdict as the undefined-Str sentinel, which the
/// accumulator would otherwise concatenate as the literal text
/// `undefined`. Every static lane answers real text (a Str element
/// goes through `json_quote_str`, which already turns its sentinel
/// into `"null"`; a `Type::Ptr` element interns `"null"` outright),
/// so only `Type::Any` needs the runtime probe — typed arrays keep
/// their branch-free loop body.
fn coerce_sentinel_to_null(ctx: &mut LowerCtx, s: Operand) -> Operand {
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_is_undef, vec![s]),
        Type::Bool,
        None,
    );
    let out = ctx.alloca_in_entry(Type::Str, Some("__json_elem_out"));
    let undef_blk = ctx.f.add_block();
    let keep_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: keep_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let null_str = ctx.intern_string_literal("null");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(null_str), Operand::Value(out), 0),
    );
    // 642 ledger — this path drops the sentinel the caller would
    // otherwise have released after its concat (a static block, so
    // the drop is a no-op; the entry keeps the account explicit).
    ctx.emit_drop_value(s, Type::Str);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = keep_blk;
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(s, Operand::Value(out), 0));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));
    ctx.cur_block = done_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(out), 0),
        Type::Str,
        None,
    );
    Operand::Value(v)
}

/// `[<e0>,<e1>,...]` — concat-loop accumulator over
/// `arr_layouts[arr_id]`-typed slots (see module doc of the parent),
/// behind the shared [`with_null_gate`] (655 arm).
pub(super) fn lower_arr(
    ctx: &mut LowerCtx,
    val_op: Operand,
    arr_id: ArrId,
    fe_elem: Option<crate::check::Type>,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    with_null_gate(ctx, &val_op.clone(), "__json_arr_out", |ctx| {
        lower_arr_walk(ctx, val_op, arr_id, fe_elem, gap, depth)
    })
}

/// ES §25.5.2.1 — `acc + "\n" + gap * depth`, the line break a
/// composite puts before each of its members under a gap. Only
/// emitted when the call site carried a `space`, so the compact
/// unfold keeps its exact instruction sequence.
pub(super) fn concat_indent(
    ctx: &mut LowerCtx,
    acc: Operand,
    gap: &Operand,
    depth: u32,
) -> Operand {
    let indent = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.json_indent,
            vec![*gap, Operand::ConstI64(depth as i64)],
        ),
        Type::Str,
        None,
    );
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_concat, vec![acc, Operand::Value(indent)]),
        Type::Str,
        None,
    );
    // 642 ledger — str_concat answers a fresh Str; both inputs are
    // consumed stakes (the indent is freshly minted, the accumulator
    // was owned by the caller).
    ctx.emit_drop_value(Operand::Value(indent), Type::Str);
    ctx.emit_drop_value(acc, Type::Str);
    Operand::Value(out)
}

/// The closing bracket's own indent, which returns to the PARENT's
/// level and is skipped entirely when the body emitted nothing —
/// `[]` and `{}` stay on one line. The "emitted anything" test is
/// the accumulator-length one the runtime helper owns.
pub(super) fn concat_close_indent(
    ctx: &mut LowerCtx,
    acc: Operand,
    gap: &Operand,
    depth: u32,
) -> Operand {
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.json_close_indent,
            vec![acc, *gap, Operand::ConstI64(depth as i64)],
        ),
        Type::Str,
        None,
    );
    Operand::Value(out)
}

/// A boolean element's JSON from its raw slot word: a hole (the word 2)
/// reads as undefined, which §25.5.2.4 step 8.a writes as `null` inside
/// an array; 0 / 1 serialize as the boolean.
fn bool_word_json(ctx: &mut LowerCtx, word: ValueId, idx_key: ValueId) -> Operand {
    let slot = ctx.alloca_in_entry(Type::Str, Some("__json_bool_elem"));
    let hole_blk = ctx.f.add_block();
    let val_blk = ctx.f.add_block();
    let join = ctx.f.add_block();
    let is_hole = ctx.cmp(IPred::Eq, Operand::Value(word), Operand::ConstI64(2));
    let cur = ctx.cur_block;
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: is_hole,
            then_blk: hole_blk,
            else_blk: val_blk,
        },
    );
    let null = ctx.intern_string_literal("null");
    ctx.f.append_void(
        hole_blk,
        InstKind::Store(Operand::Value(null), Operand::Value(slot), 0),
    );
    ctx.f.set_term(hole_blk, Terminator::Br(join));
    ctx.cur_block = val_blk;
    let b = ctx.cmp(IPred::Ne, Operand::Value(word), Operand::ConstI64(0));
    let s = super::lower_keyed(
        ctx,
        b,
        Type::Bool,
        Some(Operand::Value(idx_key)),
        None,
        None,
        0,
    );
    let cur = ctx.cur_block;
    ctx.f
        .append_void(cur, InstKind::Store(s, Operand::Value(slot), 0));
    ctx.f.set_term(cur, Terminator::Br(join));
    ctx.cur_block = join;
    Operand::Value(ctx.f.append_inst(
        join,
        InstKind::Load(Type::Str, Operand::Value(slot), 0),
        Type::Str,
        None,
    ))
}

/// The non-null walk body of [`lower_arr`] (verbatim pre-655 arm).
/// Append the element separator before every element but the first,
/// leaving `ctx.cur_block` on the join where the element itself is
/// serialized.
///
/// Split out of [`lower_arr_walk`] under the 200-line function
/// discipline — verbatim, ledger drop included.
fn emit_elem_separator(
    ctx: &mut LowerCtx,
    acc: crate::ssa::ValueId,
    comma: crate::ssa::ValueId,
    i_now: crate::ssa::ValueId,
) {
    let need_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Sgt, Operand::Value(i_now), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let sep_blk = ctx.f.add_block();
    let no_sep_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(need_sep),
            then_blk: sep_blk,
            else_blk: no_sep_blk,
        },
    );
    ctx.cur_block = sep_blk;
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let with_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(acc_now), Operand::Value(comma)],
        ),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_sep), Operand::Value(acc), 0),
    );
    // 642 ledger — release the pre-concat accumulator (str_concat
    // answers a fresh Str; the first-iteration "[" literal is a
    // FLAG_STATIC_LITERAL no-op through the same drop).
    ctx.emit_drop_value(Operand::Value(acc_now), Type::Str);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(no_sep_blk));
    ctx.cur_block = no_sep_blk;
}

fn lower_arr_walk(
    ctx: &mut LowerCtx,
    val_op: Operand,
    arr_id: ArrId,
    fe_elem: Option<crate::check::Type>,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
    let arr_ptr = match val_op {
        Operand::Value(v) => v,
        _ => unreachable!(),
    };
    let lbrack = ctx.intern_string_literal("[");
    let rbrack = ctx.intern_string_literal("]");
    let comma = ctx.intern_string_literal(",");
    let acc = ctx.alloca_in_entry(Type::Str, Some("__json_arr"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lbrack), Operand::Value(acc), 0),
    );
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let i_slot = ctx.alloca(Type::I64, Some("__json_i"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
    );
    let header_blk = ctx.f.add_block();
    let body_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = header_blk;
    let i_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
        Type::I64,
        None,
    );
    let in_bounds = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(in_bounds),
            then_blk: body_blk,
            else_blk: after_blk,
        },
    );
    ctx.cur_block = body_blk;
    emit_elem_separator(ctx, acc, comma, i_now);
    let (off_base, off) =
        ctx.emit_arr_slot_byte_offset(Operand::Value(arr_ptr), Operand::Value(i_now), 8, false);
    // A boolean element loads as its raw word — a hole holds 2.
    let load_ty = if elem_ty == Type::Bool {
        Type::I64
    } else {
        elem_ty
    };
    let elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::LoadDyn(load_ty, off_base, off),
        load_ty,
        None,
    );
    // §25.5.2 step 2.b — an element's key is its index, which this
    // lane only knows at runtime (the walk is a loop, not an unfold).
    let idx_key = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.i64_to_str, vec![Operand::Value(i_now)]),
        Type::Str,
        None,
    );
    // §25.5.2.4 step 8.a — an element that serializes to nothing is
    // `null` inside an array (an object would omit the key instead).
    // A Symbol always does, and statically so.
    let elem_str = if matches!(elem_ty, Type::Symbol) {
        Operand::Value(ctx.intern_string_literal("null"))
    } else if elem_ty == Type::Bool {
        bool_word_json(ctx, elem, idx_key)
    } else {
        let walked = super::lower_keyed(
            ctx,
            Operand::Value(elem),
            elem_ty,
            Some(Operand::Value(idx_key)),
            fe_elem.clone(),
            gap,
            depth + 1,
        );
        // A nominal element whose `toJSON` hook takes the runtime
        // walk answers the same sentinel an `any` element does.
        if matches!(elem_ty, Type::Any)
            || super::to_json::takes_runtime_walk(ctx, &elem_ty, fe_elem.as_ref())
        {
            coerce_sentinel_to_null(ctx, walked)
        } else {
            walked
        }
    };
    ctx.emit_drop_value(Operand::Value(idx_key), Type::Str);
    let acc_now2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let acc_now2 = match &gap {
        Some(g) => concat_indent(ctx, Operand::Value(acc_now2), g, depth + 1),
        None => Operand::Value(acc_now2),
    };
    let with_elem = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.str_concat, vec![acc_now2, elem_str]),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(with_elem), Operand::Value(acc), 0),
    );
    // 642 ledger — release the pre-concat accumulator and the
    // element's stringified temp (every per-elem lane answers a
    // fresh Str or an interned literal whose drop is a no-op).
    ctx.emit_drop_value(acc_now2, Type::Str);
    ctx.emit_drop_value(elem_str, Type::Str);
    let i_next = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(header_blk));
    ctx.cur_block = after_blk;
    let acc_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc), 0),
        Type::Str,
        None,
    );
    let acc_final = match &gap {
        Some(g) => concat_close_indent(ctx, Operand::Value(acc_final), g, depth),
        None => Operand::Value(acc_final),
    };
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![acc_final, Operand::Value(rbrack)],
        ),
        Type::Str,
        None,
    );
    // 642 ledger — release the final accumulator (a zero-element
    // array's acc is still the "[" literal: no-op drop).
    ctx.emit_drop_value(acc_final, Type::Str);
    Operand::Value(result)
}
