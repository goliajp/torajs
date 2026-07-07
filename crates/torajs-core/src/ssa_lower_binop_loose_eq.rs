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
use crate::ssa::{BinOp as SsaBinOp, FPred, IPred, InstKind, Operand, Type};
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
        a_is_null || ctx.binop_left_undef_id.is_some() || ctx.binop_left_null_id.is_some();
    let b_nullish =
        b_is_null || ctx.binop_right_undef_id.is_some() || ctx.binop_right_null_id.is_some();
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
            let pred = if matches!(op, AstBinOp::LooseEq) {
                IPred::Eq
            } else {
                IPred::Ne
            };
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::ICmp(pred, ptr_op, Operand::ConstPtrNull),
                Type::Bool,
                None,
            );
            return Some(Operand::Value(v));
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
    if let Some(v) = try_lower_str_num(ctx, op, a, b, a_ty, b_ty) {
        return Some(v);
    }
    if let Some(v) = try_lower_bool_str(ctx, op, a, b, a_ty, b_ty) {
        return Some(v);
    }
    None
}

/// Heap-pointer SSA types whose loose-eq-vs-nullish is a runtime
/// `ptr == null` icmp (chunk 618). `Str` rides along — the 591
/// convention stores undefined in a Str slot as NULL, and
/// `undefined == null` is true. FnSig stays out (raw fn addresses
/// are never null; the checker doesn't admit the pair yet).
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

fn try_lower_str_num(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
    a_ty: Type,
    b_ty: Type,
) -> Option<Operand> {
    let (str_op, num_op, num_ty) = if a_ty == Type::Str && matches!(b_ty, Type::I64 | Type::F64) {
        (a, b, b_ty)
    } else if b_ty == Type::Str && matches!(a_ty, Type::I64 | Type::F64) {
        (b, a, a_ty)
    } else {
        return None;
    };
    let cur_block = ctx.cur_block;
    let str_num = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_to_number, vec![str_op]),
        Type::F64,
        None,
    );
    let num_f64 = if matches!(num_ty, Type::I64) {
        ctx.coerce_to_f64(num_op)
    } else {
        num_op
    };
    let eq = ctx.fcmp(FPred::Oeq, Operand::Value(str_num), num_f64);
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let neg = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Xor, eq, Operand::ConstBool(true)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(neg));
    }
    Some(eq)
}

fn try_lower_bool_str(
    ctx: &mut LowerCtx<'_>,
    op: AstBinOp,
    a: Operand,
    b: Operand,
    a_ty: Type,
    b_ty: Type,
) -> Option<Operand> {
    let (bool_op, str_op) = if a_ty == Type::Bool && b_ty == Type::Str {
        (a, b)
    } else if a_ty == Type::Str && b_ty == Type::Bool {
        (b, a)
    } else {
        return None;
    };
    let cur_block = ctx.cur_block;
    let bool_i64 = ctx
        .f
        .append_inst(cur_block, InstKind::ZExtBoolToI64(bool_op), Type::I64, None);
    let bool_f64 = ctx.coerce_to_f64(Operand::Value(bool_i64));
    let cur_block = ctx.cur_block;
    let str_num = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.str_to_number, vec![str_op]),
        Type::F64,
        None,
    );
    let eq = ctx.fcmp(FPred::Oeq, bool_f64, Operand::Value(str_num));
    if matches!(op, AstBinOp::LooseNeq) {
        let cur_block = ctx.cur_block;
        let neg = ctx.f.append_inst(
            cur_block,
            InstKind::BinOp(SsaBinOp::Xor, eq, Operand::ConstBool(true)),
            Type::Bool,
            None,
        );
        return Some(Operand::Value(neg));
    }
    Some(eq)
}
