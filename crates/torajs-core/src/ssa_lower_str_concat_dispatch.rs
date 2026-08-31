//! `<Str>.concat | <Arr>.concat` dispatch — seventh sub-split
//! carved out of [`ssa_lower_str::try_lower_method_call`]. Both
//! `concat` shapes lower as variadic left-folds over a 2-operand
//! intrinsic (`str_concat` / `arr_concat`), so they share the
//! intermediate-value drop discipline and live together here.
//!
//! Returns `None` for non-Str / non-Arr receivers or for the
//! `method != "concat"` case so the caller can keep trying the
//! remaining branches.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Str>.concat(...)` or `<Arr>.concat(...)` through
/// the variadic left-fold dispatch. Returns `Some(value)` when
/// handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `s.concat(...others)` — variadic string concat,
    // lowered as a left-fold over str_concat. Empty arg
    // list returns the receiver unchanged. The single-arg
    // case still flows through the typecheck Function-arm
    // dispatch but we intercept here uniformly to avoid
    // duplicate emit paths.
    if recv_ty == Type::Str && method == "concat" {
        if args.is_empty() {
            // RFC 20260705 owned-result invariant: even the identity
            // shape answers an owned ref.
            ctx.emit_rc_inc(recv_op.clone());
            return Some(recv_op);
        }
        let mut acc = recv_op;
        // RFC 20260705 chunk 546 — the left-fold mints a fresh Str
        // per round; every non-final acc is a temp the next concat
        // only borrows. Drop it post-concat (round 0's acc is the
        // caller-owned receiver — untouched).
        let mut acc_fresh = false;
        for &a in args {
            // S212 — explicit `undefined` arg per ES §22.1.3.4
            // step 3.a: each arg is ToString'd, undefined →
            // "undefined". Inline-substitute the interned
            // literal so the helper sees a valid Str pointer
            // — same idiom S207/S211 use for replace/locale-
            // Compare.
            let mut other_fresh = false;
            let other = if matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined)) {
                let u = ctx.intern_string_literal("undefined");
                Operand::Value(u)
            } else {
                let v = ctx.lower_expr(a);
                // §22.1.3.5 step 3.b — ToString each argument. An
                // Any actual (the checker's any→Str admit: a String
                // wrapper object, a boxed primitive) carries a
                // NaN-box the raw str_concat kernel would deref as a
                // Str pointer (SIGSEGV on `'a'.concat(Object('b'))`)
                // — route it through the ToString kernel, which
                // answers an owned Str (html-wrap lane idiom).
                if ctx.operand_ty(&v) == Type::Any {
                    let s = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.any_to_str_box, vec![v]),
                        Type::Str,
                        None,
                    );
                    ctx.emit_throw_check(None);
                    other_fresh = true;
                    Operand::Value(s)
                } else {
                    v
                }
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_concat, vec![acc.clone(), other.clone()]),
                Type::Str,
                None,
            );
            if acc_fresh {
                ctx.emit_drop_value(acc, Type::Str);
            }
            if other_fresh {
                ctx.emit_drop_value(other, Type::Str);
            }
            acc = Operand::Value(v);
            acc_fresh = true;
        }
        return Some(acc);
    }
    // `arr.concat(other)` — fresh array, single malloc +
    // two memcpys via the C runtime. Element type carried.
    // Phase B refcount: derived array's slots alias both
    // sources; inc each slot for non-Copy elements.
    //
    // V3-18 wedge — multi-arg form `xs.concat(a, b, ..., z)`
    // per JS spec §22.1.3.2 is supported by folding the
    // single-arg intrinsic left-to-right: each step's
    // result becomes the next step's receiver. Refcount
    // inc runs once at the end over the final array's
    // full length. Each intermediate also leaks otherwise;
    // those temporaries are drop-balanced by the rc-inc
    // window on the final result (intermediates aren't
    // bound to a name so the surrounding scope-end drop
    // doesn't see them).
    if let Type::Arr(arr_id) = recv_ty
        && method == "concat"
    {
        return Some(lower_arr_concat(ctx, call_eid, recv_op, arr_id, args));
    }
    None
}

/// `<Arr<Any>>.concat(...)` — dedicated lane over the
/// FLAG_ARR_ANY-aware runtime family. The typed lane's helpers are
/// flag-blind (`arr_slice` / `arr_concat` products lose FLAG_ARR_ANY
/// → the drop walker never decs the NaN-box elements; a scalar arg
/// reaches `arr_concat` as a fake array pointer → SIGSEGV).
///
/// rc ledger — every step incs the slots it writes itself
/// (`arr_any_slice` seed, `arr_extend_any`, the in-place
/// `arr_extend_typed_into_any`, `pack_any_elem`), so no trailing raw
/// inc walk runs here (it would double-inc, and raw `rc_inc` over
/// NaN-box bits only survives via the cell-like guard). Owned-temp
/// args release their own stake post-step (share contract, RFC
/// 20260705).
fn lower_concat_any_recv(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    arr_id: crate::ssa::ArrId,
    args: &[ExprId],
) -> Operand {
    // Seed: fresh flag-aware shallow copy — ES §23.1.3.2 concat
    // never mutates the receiver, and the extend helpers below
    // append in place.
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv_op.clone(), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let seed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_any_slice,
            vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
        ),
        Type::Arr(arr_id),
        None,
    );
    extend_any_acc_with_args(ctx, arr_id, Operand::Value(seed), args)
}

/// The §23.1.3.2 per-argument fold shared by the two lanes that
/// build an `Arr<Any>` result (`Arr<Any>` receiver above, mixed
/// typed receiver below): an `Arr<Any>` arg extends NaN-box slots,
/// a typed array boxes per elem tag, anything else appends as one
/// packed element. Every step incs the slots it writes itself.
fn extend_any_acc_with_args(
    ctx: &mut LowerCtx<'_>,
    arr_id: crate::ssa::ArrId,
    mut acc: Operand,
    args: &[ExprId],
) -> Operand {
    for &a in args {
        let other = ctx.lower_expr(a);
        let other_ty = ctx.operand_ty(&other);
        let transfers = ctx.expr_transfers_ownership(a);
        let v = match other_ty {
            Type::Arr(oid) => {
                let oet = ctx.arr_layouts[oid.0 as usize];
                if matches!(oet, Type::Any) {
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(ctx.intrinsics.arr_extend_any, vec![acc, other.clone()]),
                        Type::Arr(arr_id),
                        None,
                    )
                } else {
                    // Rotation 545 — a typed arg's elements cross
                    // into the any world here; when they are
                    // themselves arrays, the nested cells must be
                    // kind-marked or every kind-aware reader answers
                    // null (`[1].concat([[2]])` printed `[1,[null]]`).
                    // Self-gates: chain 0 for scalar elems.
                    ctx.emit_arr_mark_kind(&other);
                    let elem_tag = any_elem_tag(oet);
                    ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::Call(
                            ctx.intrinsics.arr_extend_typed_into_any,
                            vec![acc, other.clone(), Operand::ConstI64(elem_tag)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    )
                }
            }
            // Rotation 546 — an Any argument's §23.1.3.1
            // spread-vs-append question is a runtime is-array test;
            // the kernel borrows the box and takes its own stakes
            // per slot. This arm previously fell to the packed
            // single-element append below — a silent wrong for an
            // Any holding an array.
            Type::Any => ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_concat_any_arg, vec![acc, other.clone()]),
                Type::Arr(arr_id),
                None,
            ),
            _ => {
                // ES §23.1.3.2 — non-array arg appends as a single
                // element. pack_any_elem incs refcounted values /
                // unboxes Any pairs; arr_push_any adopts the pair.
                let (tag_op, value_op) = ctx.pack_any_elem(other.clone(), other_ty, Some(a));
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_push_any, vec![acc, tag_op, value_op]),
                    Type::Arr(arr_id),
                    None,
                )
            }
        };
        acc = Operand::Value(v);
        // Owned-temp arg: the slot took its own inc'd stake above —
        // release the temp's stake so it doesn't orphan.
        if transfers && other_ty.is_refcounted() {
            ctx.emit_drop_value(other, other_ty);
        }
    }
    acc
}

/// NaN-box elem tag for a typed array's element type — the scheme
/// `__torajs_arr_extend_typed_into_any` boxes by.
fn any_elem_tag(t: Type) -> i64 {
    match t {
        Type::Bool => 1,
        Type::I64 | Type::I32 => 2,
        Type::F64 => 3,
        t if t.is_refcounted() => 4,
        other => panic!("ssa-lower: Array<Any>.concat typed-arg elem {other:?} not supported"),
    }
}

/// Rotation 545 — `<Arr<T>>.concat(...)` whose checked result is
/// `Array<Any>`: §23.1.3.1 answers a heterogeneous element set when
/// a statically-shaped argument diverges from T. Seed an `Arr<Any>`
/// with the receiver's elements boxed per tag (the kernel
/// materializes inline Substr views — rotation 468), then run the
/// same per-argument fold the `Arr<Any>` receiver takes.
fn lower_concat_mixed(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    recv_elem: Type,
    args: &[ExprId],
) -> Operand {
    let any_id = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Any);
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv_op.clone(), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let seed = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.arr_alloc_any, vec![Operand::Value(len)]),
        Type::Arr(any_id),
        None,
    );
    // Nested receiver elements cross into the any world — kind-mark
    // them (self-gates; chain 0 for scalar elems).
    ctx.emit_arr_mark_kind(&recv_op);
    let elem_tag = any_elem_tag(recv_elem);
    let seeded = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_extend_typed_into_any,
            vec![Operand::Value(seed), recv_op, Operand::ConstI64(elem_tag)],
        ),
        Type::Arr(any_id),
        None,
    );
    extend_any_acc_with_args(ctx, any_id, Operand::Value(seeded), args)
}

/// `<Arr>.concat(...)` — ES §23.1.3.2: every argument is either an array
/// (spread into the result) or a single element (appended); multi-arg
/// folds the single-arg kernel left to right. The result owns its
/// slots after one ownership walk at the end. Carved out of
/// [`try_dispatch`] when the view-source bookkeeping pushed it past
/// the 200-line function limit (rotation 468).
fn lower_arr_concat(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    recv_op: Operand,
    arr_id: crate::ssa::ArrId,
    args: &[ExprId],
) -> Operand {
    // §23.1.3.1 step 2 ArraySpeciesCreate — constructor-face
    // guard before the derive (RFC 20260713 blade 3).
    ctx.emit_arr_species_guard(recv_op.clone());
    // Any receiver — every step must stay FLAG_ARR_ANY-aware
    // (raw arr_slice / arr_concat products are flag-blind and a
    // scalar arg would be deref'd as an array pointer). Dedicated
    // lane below.
    if matches!(ctx.arr_layouts[arr_id.0 as usize], Type::Any) {
        return lower_concat_any_recv(ctx, recv_op, arr_id, args);
    }
    // Rotation 545 — the checker types a heterogeneous concat
    // `Array<Any>` (§23.1.3.1); this lane reads that verdict back
    // rather than re-deriving it, so the two faces cannot disagree
    // (rotation 544's mixed-anchor lesson). Typed receiver + Any
    // call type has exactly one source: the mixed arm.
    if matches!(
        ctx.expr_types.get(&call_eid),
        Some(crate::check::Type::Array(e)) if matches!(**e, crate::check::Type::Any)
    ) {
        let recv_elem = ctx.arr_layouts[arr_id.0 as usize];
        return lower_concat_mixed(ctx, recv_op, recv_elem, args);
    }
    // 0-arg form ≡ shallow copy. Lower as
    // `arr_slice(recv, 0, len)` — the refcount-inc
    // walk below handles non-Copy elements the
    // same way as for slice / concat results.
    // A view does not leave its split block (rotation 468): a
    // concat that copies out of an `Arr<Substr>` — the receiver,
    // an array argument, or a lone view argument — answers
    // `Arr<Str>`, and the ownership walk at the end materializes
    // every view it copied. `saw_views` remembers whether any
    // source was view-typed; `out_elem` is the product's element
    // type (the receiver's, unless that is Substr).
    let recv_elem = ctx.arr_layouts[arr_id.0 as usize];
    let mut saw_views = recv_elem == Type::Substr;
    let out_id = ctx.copied_arr_layout(arr_id);
    let out_elem = ctx.arr_layouts[out_id.0 as usize];
    let mut acc = if args.is_empty() {
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_slice,
                vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(out_id),
            None,
        );
        Operand::Value(v)
    } else {
        recv_op
    };
    // ES §23.1.3.2 — concat returns a fresh array; receiver must
    // not be mutated. `arr_concat` always allocates a new buffer,
    // but `arr_push` (used below for scalar args) may mutate
    // in-place when capacity allows. Track when `acc` is still
    // aliased to the receiver and force a shallow copy before the
    // first scalar push to preserve spec semantics.
    let mut acc_is_fresh = args.is_empty();
    for a in args {
        let other = ctx.lower_expr(*a);
        let other_ty = ctx.operand_ty(&other);
        // A lone view argument is appended as an owned copy; a
        // fresh mint (index / method) hands this lane its only
        // ref, a borrow stays with its owner (the push arm's rule).
        let (other, other_ty) = if other_ty == Type::Substr && out_elem == Type::Str {
            let owned = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![other.clone()]),
                Type::Str,
                None,
            );
            if ctx.expr_transfers_ownership(*a) {
                ctx.emit_drop_value(other, Type::Substr);
            }
            (Operand::Value(owned), Type::Str)
        } else {
            (other, other_ty)
        };
        if let Type::Arr(oid) = other_ty
            && ctx.arr_layouts[oid.0 as usize] == Type::Substr
        {
            saw_views = true;
        }
        // ES §23.1.3.2 — scalar arg (same type as receiver elem)
        // is appended as a single element instead of spread.
        if other_ty == out_elem && !matches!(other_ty, Type::Arr(_)) {
            if !acc_is_fresh {
                let len = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::I64, acc, ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_slice,
                        vec![acc, Operand::ConstI64(0), Operand::Value(len)],
                    ),
                    Type::Arr(out_id),
                    None,
                );
                acc = Operand::Value(v);
                acc_is_fresh = true;
            }
            let push_arg = ctx.raw_slot_arg(other);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_push, vec![acc, push_arg]),
                Type::Arr(out_id),
                None,
            );
            acc = Operand::Value(v);
            continue;
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_concat, vec![acc, other]),
            Type::Arr(out_id),
            None,
        );
        acc = Operand::Value(v);
        // arr_concat returns a new ptr — acc is now detached
        // from the receiver buffer.
        acc_is_fresh = true;
    }
    if out_elem.is_refcounted() {
        let len = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, acc, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        // Every slot of the product was memcpy'd from a source and
        // owns nothing yet. With a view-typed source anywhere, the
        // adopt kernel materializes each view and shares each
        // owned string; otherwise the plain rc-inc walk.
        let copied_from = if saw_views { Type::Substr } else { out_elem };
        ctx.emit_adopt_copied_range(acc, copied_from, Operand::ConstI64(0), Operand::Value(len));
    }
    acc
}
