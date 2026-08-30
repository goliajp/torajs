//! `LooseEq` / `LooseNeq` short-circuit folds pulled out of
//! [`crate::ssa_lower::lower_binop_inner`]'s leading guard as
//! chunk-85 of the decomp.
//!
//! Three short-circuit paths claim the `==` / `!=` cross-type
//! cases that the generic coerce_op path would mishandle (null
//! must NOT coerce to 0 per spec §7.2.13). Returns `Some` on a
//! match; falls through (`None`) so the generic binop path
//! handles the remaining (non-null, non-mixed-string-number) pairs.
//!
//! - **S127-2** — `(Any, null)` / `(null, Any)`: per §7.2.13 step 2,
//!   nullish loose-equals nullish. Emit
//!   `or(any_strict_eq(v, ANY_NULL, 0), any_strict_eq(v, ANY_UNDEF, 0))`
//!   using existing intrinsics (no new runtime helper). Common idiom
//!   in callbacks over `Array<Any>` (`.some` / `.every` / etc).
//! - **`(null, null)` static fold** — both sides ConstPtrNull: emit
//!   ConstBool depending on op.
//! - **§7.2.13 step 6** — `String == Number` / `Number == String`:
//!   coerce the string side via `__torajs_str_to_number` and compare
//!   numerically. `'1' == 1` → true; `'abc' == 0` → NaN == 0 → false.
//! - **§7.2.13 steps 7-9** — `Boolean == String` / `String == Boolean`:
//!   coerce the Boolean side to Number (`true` → 1, `false` → 0)
//!   then numerical compare against `str_to_number`.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::LooseEq | AstBinOp::LooseNeq) {
        return None;
    }
    // Chunk 612 — nullish is a TYPE property, not an operand shape:
    // an Undefined/Null-typed binding is a Load, not ConstPtrNull.
    // §7.2.13 step 2: nullish == nullish → true regardless of which
    // of the two nullish values each side holds.
    let a_is_null = matches!(a, Operand::ConstPtrNull);
    let b_is_null = matches!(b, Operand::ConstPtrNull);
    let a_nullish =
        a_is_null || ctx.binop.left_undef_id.is_some() || ctx.binop.left_null_id.is_some();
    let b_nullish =
        b_is_null || ctx.binop.right_undef_id.is_some() || ctx.binop.right_null_id.is_some();
    if a_nullish ^ b_nullish
        && let Some(v) = try_lower_any_loose_eq_null(ctx, op, a.clone(), b.clone(), a_nullish)
    {
        return Some(v);
    }
    // Chunk 618 — heap-reference × nullish compares the POINTER at
    // runtime: a Nullable narrow can leak a nullish value into a
    // bare heap-typed binding (p2 probe), and a NULL Str slot means
    // undefined by the 591 convention — one icmp is correct in
    // every case, while the static cross-type-false fold below is
    // only sound for value types.
    if a_nullish ^ b_nullish {
        let ptr_op = if a_nullish { b.clone() } else { a.clone() };
        let ptr_ty = ctx.operand_ty(&ptr_op);
        // RFC 20260722 chunk D — an undefable F64 side (number[]
        // find miss / index OOB / alias) vs a nullish literal: the
        // sentinel bits ARE `undefined`, loose-equal to both null
        // and undefined per §7.2.13 steps 2-3. Plain F64s keep the
        // static cross-type-false fold below.
        if ptr_ty == Type::F64 {
            let undefable = if a_nullish {
                ctx.binop.right_f64_undefable
            } else {
                ctx.binop.left_f64_undefable
            };
            if undefable {
                let bits = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::BitCastF64ToI64(ptr_op),
                    Type::I64,
                    None,
                );
                let cmp = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::ICmp(
                        IPred::Eq,
                        Operand::Value(bits),
                        Operand::ConstI64(
                            crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64,
                        ),
                    ),
                    Type::Bool,
                    None,
                );
                if matches!(op, AstBinOp::LooseNeq) {
                    let r = ctx.f.append_inst(
                        ctx.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Xor,
                            Operand::Value(cmp),
                            Operand::ConstBool(true),
                        ),
                        Type::Bool,
                        None,
                    );
                    return Some(Operand::Value(r));
                }
                return Some(Operand::Value(cmp));
            }
        }
        // RFC 20260707 chunk 2 (+ residual chunk) — a Str slot has
        // TWO nullish reprs: NULL (JS null) and the undefined
        // sentinel cell (missed exec/match capture); a Substr slot
        // has the Substr-shaped sentinel (string index OOB read).
        // `s == null` / `s == undefined` is true for all of them
        // (§7.2.13 steps 2-3) — probe via the runtime
        // `str_is_nullish` (ptr==0 || either sentinel address).
        if matches!(ptr_ty, Type::Str | Type::Substr) {
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.str_is_nullish, vec![ptr_op]),
                Type::Bool,
                None,
            );
            if matches!(op, AstBinOp::LooseNeq) {
                let r = ctx.f.append_inst(
                    cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, Operand::Value(v), Operand::ConstBool(true)),
                    Type::Bool,
                    None,
                );
                return Some(Operand::Value(r));
            }
            return Some(Operand::Value(v));
        }
        if is_heap_ref(&ptr_ty) {
            // RFC 20260722 chunk C — a sentinel-capable slot
            // (Obj/Arr/Closure/FnSig) has TWO nullish reprs: NULL
            // (JS null) and the generic undefined cell (Nullable
            // field read / find miss); `x == null` / `== undefined`
            // is true for both (§7.2.13 steps 2-3). Two inline cmps
            // + or, zero calls; sentinel-less types keep the plain
            // NULL cmp.
            let cur_block = ctx.cur_block;
            let eq_null = ctx.f.append_inst(
                cur_block,
                InstKind::ICmp(IPred::Eq, ptr_op.clone(), Operand::ConstPtrNull),
                Type::Bool,
                None,
            );
            let nullish = if let Some(sentinel) = ctx.str_undef_sentinel_for(ptr_ty) {
                let cur_block = ctx.cur_block;
                let eq_undef = ctx.f.append_inst(
                    cur_block,
                    InstKind::ICmp(IPred::Eq, ptr_op, sentinel),
                    Type::Bool,
                    None,
                );
                let v = ctx.f.append_inst(
                    cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Or,
                        Operand::Value(eq_null),
                        Operand::Value(eq_undef),
                    ),
                    Type::Bool,
                    None,
                );
                Operand::Value(v)
            } else {
                Operand::Value(eq_null)
            };
            if matches!(op, AstBinOp::LooseNeq) {
                let cur_block = ctx.cur_block;
                let r = ctx.f.append_inst(
                    cur_block,
                    InstKind::BinOp(SsaBinOp::Xor, nullish, Operand::ConstBool(true)),
                    Type::Bool,
                    None,
                );
                return Some(Operand::Value(r));
            }
            return Some(nullish);
        }
    }
    if a_nullish || b_nullish {
        let result = a_nullish && b_nullish;
        let answer = match op {
            AstBinOp::LooseEq => result,
            AstBinOp::LooseNeq => !result,
            _ => unreachable!(),
        };
        return Some(Operand::ConstBool(answer));
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    if let Some(v) =
        crate::ssa_lower_binop_loose_eq_num::try_lower_str_num(ctx, op, a, b, a_ty, b_ty)
    {
        return Some(v);
    }
    if let Some(v) =
        crate::ssa_lower_binop_loose_eq_num::try_lower_bool_str(ctx, op, a, b, a_ty, b_ty)
    {
        return Some(v);
    }
    if let Some(v) = try_lower_any_ladder(ctx, op, a, b) {
        return Some(v);
    }
    None
}

/// RFC 20260713-loose-eq-substrate blade 1 — either side `Any` (or,
/// blade 2, a static BigInt-mix pair) and none of the earlier arms
/// hit (nullish shapes are folded above): route through the runtime
/// §7.2.14 ladder (`__torajs_anyv_loose_eq`). The concrete side
/// boxes to an AnyValue borrow — Str/Substr via the sentinel-aware
/// str-slot boxer (NULL → null / undef-cell → undefined), everything
/// else through the `(tag, value)` pair encoder. A ToPrimitive
/// coercion on an object operand can raise a pending TypeError, so
/// the call is followed by a throw check.
fn try_lower_any_ladder(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    // Blade 2 — static BigInt pairs: BigInt × BigInt loose-eq must
    // compare mathematical values (the generic path compared
    // pointers), and BigInt × {num, bool, str} walks the same
    // ladder arms the Any route uses.
    let bigint_mix = |x: &Type, y: &Type| {
        matches!(x, Type::BigInt)
            && matches!(
                y,
                Type::BigInt
                    | Type::I64
                    | Type::I32
                    | Type::F64
                    | Type::Bool
                    | Type::Str
                    | Type::Substr
            )
    };
    // Blade 3 — C3: a same-type object pair compares by pointer
    // identity (§7.2.14 step 1 → IsStrictlyEqual); one icmp beats
    // the box-and-call round trip.
    if a_ty == b_ty && is_heap_obj(&a_ty) {
        let pred = if matches!(op, AstBinOp::LooseEq) {
            IPred::Eq
        } else {
            IPred::Ne
        };
        let cur_block = ctx.cur_block;
        let v = ctx
            .f
            .append_inst(cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        return Some(Operand::Value(v));
    }
    // Blade 3 — C4: object × primitive walks ToPrimitive through
    // the ladder (§7.2.14 steps 11-12).
    let obj_prim_mix = |x: &Type, y: &Type| {
        is_heap_obj(x)
            && matches!(
                y,
                Type::I64
                    | Type::I32
                    | Type::F64
                    | Type::Bool
                    | Type::Str
                    | Type::Substr
                    | Type::BigInt
            )
    };
    if !matches!(a_ty, Type::Any)
        && !matches!(b_ty, Type::Any)
        && !bigint_mix(&a_ty, &b_ty)
        && !bigint_mix(&b_ty, &a_ty)
        && !obj_prim_mix(&a_ty, &b_ty)
        && !obj_prim_mix(&b_ty, &a_ty)
    {
        return None;
    }
    let l = ensure_anyv(ctx, a, a_ty);
    let r = ensure_anyv(ctx, b, b_ty);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_any_loose_eq, vec![l, r]),
        Type::Bool,
        None,
    );
    ctx.emit_throw_check(None);
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let neg = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Xor, Operand::Value(v), Operand::ConstBool(true)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(neg));
    }
    Some(Operand::Value(v))
}

/// Coerce a concrete-typed operand into a borrow-shaped AnyValue
/// for the loose-eq ladder; an Any operand passes through.
fn ensure_anyv(ctx: &mut LowerCtx<'_>, v: Operand, ty: Type) -> Operand {
    if matches!(ty, Type::Any) {
        return v;
    }
    let cur_block = ctx.cur_block;
    if matches!(ty, Type::Str | Type::Substr) {
        let b = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.anyv_box_str_slot, vec![v]),
            Type::Any,
            None,
        );
        return Operand::Value(b);
    }
    // A typed Arr crossing into the anyv ladder must be
    // self-describing — without the kind mark the helper's
    // ToPrimitive join reads elements under the UNSET contract
    // (undefined) and `[1] == 1` answers false. No-op for
    // non-Arr types and Arr<Any>.
    ctx.emit_arr_mark_kind(&v);
    let (tag, value) = ctx.pack_any_slot_value(&v, ty, false);
    let cur_block = ctx.cur_block;
    let b = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_box, vec![tag, value]),
        Type::Any,
        None,
    );
    Operand::Value(b)
}

/// Heap-pointer SSA types whose loose-eq-vs-nullish is a runtime
/// `ptr == null` icmp (chunk 618). `Str` rides along — the 591
/// convention stores undefined in a Str slot as NULL, and
/// `undefined == null` is true. FnSig stays out (raw fn addresses
/// are never null; the checker doesn't admit the pair yet).
/// Heap-object SSA types whose same-type loose-eq is pointer
/// identity and whose primitive mixes walk ToPrimitive (blade 3).
/// Str/Substr stay out (string bucket — byte equality), BigInt
/// stays out (mathematical-value bucket), Ptr stays out (nullish
/// carrier, folded earlier).
fn is_heap_obj(ty: &Type) -> bool {
    is_heap_ref(ty) && !matches!(ty, Type::Str | Type::Substr | Type::BigInt | Type::Ptr)
}

fn is_heap_ref(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Str
            | Type::Substr
            | Type::Obj(_)
            | Type::Arr(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::BigInt
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
            | Type::Map
            | Type::Set
            | Type::MapIter
            | Type::ArrIter
            | Type::Ptr
    )
}

fn try_lower_any_loose_eq_null(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
    a_is_null: bool,
) -> Option<Operand> {
    let any_op = if a_is_null { b } else { a };
    let any_ty = ctx.operand_ty(&any_op);
    if !matches!(any_ty, Type::Any) {
        return None;
    }
    let cur_block = ctx.cur_block;
    let eq_null = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_strict_eq,
            vec![any_op, Operand::ConstI64(0), Operand::ConstI64(0)],
        ),
        Type::Bool,
        None,
    );
    let cur_block = ctx.cur_block;
    let eq_undef = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_strict_eq,
            vec![any_op, Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Bool,
        None,
    );
    let cur_block = ctx.cur_block;
    let or_v = ctx.f.append_inst(
        cur_block,
        InstKind::BinOp(
            SsaBinOp::Or,
            Operand::Value(eq_null),
            Operand::Value(eq_undef),
        ),
        Type::Bool,
        None,
    );
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let inv = ctx.f.append_inst(
            cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(or_v), Operand::ConstBool(false)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(inv));
    }
    Some(Operand::Value(or_v))
}
