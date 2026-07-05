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
        for &a in args {
            // S212 — explicit `undefined` arg per ES §22.1.3.4
            // step 3.a: each arg is ToString'd, undefined →
            // "undefined". Inline-substitute the interned
            // literal so the helper sees a valid Str pointer
            // — same idiom S207/S211 use for replace/locale-
            // Compare.
            let other = if matches!(ctx.expr_types.get(&a), Some(crate::check::Type::Undefined)) {
                let u = ctx.intern_string_literal("undefined");
                Operand::Value(u)
            } else {
                ctx.lower_expr(a)
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.str_concat, vec![acc, other]),
                Type::Str,
                None,
            );
            acc = Operand::Value(v);
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
        // 0-arg form ≡ shallow copy. Lower as
        // `arr_slice(recv, 0, len)` — the refcount-inc
        // walk below handles non-Copy elements the
        // same way as for slice / concat results.
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
                Type::Arr(arr_id),
                None,
            );
            Operand::Value(v)
        } else {
            recv_op
        };
        let recv_elem = ctx.arr_layouts[arr_id.0 as usize];
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
            // ES §23.1.3.2 — scalar arg (same type as receiver elem)
            // is appended as a single element instead of spread.
            if other_ty == recv_elem && !matches!(other_ty, Type::Arr(_)) {
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
                        Type::Arr(arr_id),
                        None,
                    );
                    acc = Operand::Value(v);
                    acc_is_fresh = true;
                }
                let push_arg = ctx.raw_slot_arg(other);
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_push, vec![acc, push_arg]),
                    Type::Arr(arr_id),
                    None,
                );
                acc = Operand::Value(v);
                continue;
            }
            // S129-4 Array<Any>.concat(Array<typed>) — receiver elem
            // is Any (NaN-box AnyValue per slot), arg is a typed
            // Array<T>. The classic arr_concat is a raw 8B-stride
            // memcpy which would copy T's raw bits straight into
            // Array<Any> slots — wrong (NaN-box expects tag/value
            // pairs). Route to arr_extend_typed_into_any with the
            // T-derived elem_tag so the runtime pairs each raw slot
            // with the right ANY_* tag before append. Heap T's
            // rc_inc is handled inside the helper. Same Array<Any>
            // mixed-typed escape series as S128-1..3 push / fill.
            let typed_into_any = matches!(recv_elem, Type::Any)
                && matches!(other_ty, Type::Arr(oid) if !matches!(ctx.arr_layouts[oid.0 as usize], Type::Any));
            let v = if typed_into_any {
                let Type::Arr(oid) = other_ty else {
                    unreachable!()
                };
                let oet = ctx.arr_layouts[oid.0 as usize];
                let elem_tag = match oet {
                    Type::Bool => 1,
                    Type::I64 | Type::I32 => 2,
                    Type::F64 => 3,
                    t if t.is_refcounted() => 4,
                    other => panic!(
                        "ssa-lower: Array<Any>.concat typed-arg elem {other:?} not supported"
                    ),
                };
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_extend_typed_into_any,
                        vec![acc, other, Operand::ConstI64(elem_tag)],
                    ),
                    Type::Arr(arr_id),
                    None,
                )
            } else {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_concat, vec![acc, other]),
                    Type::Arr(arr_id),
                    None,
                )
            };
            acc = Operand::Value(v);
            // arr_concat / arr_extend_typed_into_any both return new
            // ptrs — acc is now detached from the receiver buffer.
            acc_is_fresh = true;
        }
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let len = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, acc, ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(acc, Operand::ConstI64(0), Operand::Value(len));
        }
        return Some(acc);
    }
    None
}
