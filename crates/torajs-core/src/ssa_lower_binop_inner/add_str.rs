//! `+` string-concat family of `lower_binop_inner` — Undefined-
//! side resolution, Substr view-aware concat, Str×Str concat, and
//! the mixed Number/Bool/Null/BigInt/Arr/Obj ToString-then-concat
//! path. Split from `ssa_lower_binop_inner.rs` (2026-07-03,
//! fn-debt decomp) as a `try_lower` sibling mirroring the
//! `binop_inner_{any_arith,strict_eq,bigint,str_cmp,f64,i64}`
//! family. Bodies verbatim; mechanical rewrites: matched-path
//! `return Operand::Value(v)` → `Some(..)`, the fall-through tail
//! becomes `None`, and the `coerce` closure hoists to the
//! file-local [`coerce_to_str`] fn.

use crate::ast::BinOp as AstBinOp;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

// The per-operand ToString family lives in the sibling; these two
// names keep the paths their out-of-module callers already use
// (`ssa_lower_str_html`, `ssa_lower_call_coercion_str`).
use super::add_str_coerce::{coerce_nullable_ptr_to_str, ptr_shaped};
pub(crate) use super::add_str_coerce::{coerce_to_str, coerce_undefable_f64};

pub(crate) fn try_lower(
    ctx: &mut LowerCtx,
    op: AstBinOp,
    a: Operand,
    b: Operand,
) -> Option<Operand> {
    if !matches!(op, AstBinOp::Add) {
        return None;
    }
    // S142 — String + Undefined per ES §13.15.3. Undefined lowers
    // to ConstPtrNull (same i64-0 ABI as Null), so the bool/null
    // detection downstream can't distinguish the two from operand
    // shape alone. Resolve the Undefined side here via the
    // `binop_*_undef_id` hint set by lower_binop_with_ids; emit
    // `__torajs_undefined_to_str()` and replace the operand with
    // the resulting Str so the str+str fast path picks it up.
    // Guard on the *other* side being string-shaped so numeric
    // `undefined + 0` (spec: NaN) keeps its current behavior.
    let mut a = a;
    let mut b = b;
    // RFC 20260705 chunk 546 — operands minted by this lowering
    // (undefined_to_str here, coerce_to_str temps below) are fresh
    // owned Strs the concat only borrows; drop them after the
    // concat call or they leak per evaluation.
    let mut a_temp = false;
    let mut b_temp = false;
    let str_shaped = |t: Type| matches!(t, Type::Str | Type::Substr);
    if ctx.binop.left_undef_id.is_some() && str_shaped(ctx.operand_ty(&b)) {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.undefined_to_str, vec![]),
            Type::Str,
            None,
        );
        a = Operand::Value(v);
        a_temp = true;
    }
    if ctx.binop.right_undef_id.is_some() && str_shaped(ctx.operand_ty(&a)) {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.undefined_to_str, vec![]),
            Type::Str,
            None,
        );
        b = Operand::Value(v);
        b_temp = true;
    }
    let a_ty = ctx.operand_ty(&a);
    let b_ty = ctx.operand_ty(&b);
    // V3-18 m1.d / m3.c — string concat with Bool / Null /
    // BigInt on either side. ssa_lower coerces via
    // __torajs_bool_to_str / __torajs_null_to_str /
    // __torajs_bigint_to_string before concat.
    // The null side is recognised by its TYPE, not by the operand
    // being the literal: `const s = null; "x" + s` binds the value, so
    // the read is a Load and this gate used to answer false — the pair
    // then fell past the concat coerce and printed the POINTER
    // (`x4305973960` where bun prints `xnull`). A bare `Type::Ptr`
    // here is null and nothing else; every pointer-shaped VALUE
    // carries its own SSA type and the nullable ones route through
    // `coerce_nullable_ptr_to_str` above.
    let bool_or_null = |t: Type, _op: &Operand| -> bool { matches!(t, Type::Bool | Type::Ptr) };
    let str_or_substr = |t: Type| matches!(t, Type::Str | Type::Substr);
    // S138 — `String + Arr` / `String + Obj` (ES §13.15.3
    // ToPrimitive(Default) → ToString on the non-String side).
    // Mirror of the explicit `String(arr) / String(struct)`
    // S137 coerce — routes Arr through arr_join(",") and Obj
    // through the `"[object Object]"` literal.
    let arr_or_obj = |t: Type| matches!(t, Type::Arr(_) | Type::Obj(_));
    // RFC 20260719-fn-tostring-source B5 — Str + fn concat routes
    // the fn side through the erased-source toString kernels.
    let fn_like = |t: Type| matches!(t, Type::FnSig(_) | Type::Closure(_));
    // Either operand may be a pointer-shaped nullable, whose null is
    // the in-band 0 — see `coerce_nullable_ptr_to_str`.
    let nullable_side = ctx.binop.left_nullable_ptr || ctx.binop.right_nullable_ptr;
    let mixed_string = matches!(
        (a_ty, b_ty),
        (Type::Str, Type::I64)
            | (Type::Str, Type::F64)
            | (Type::Str, Type::BigInt)
            | (Type::I64, Type::Str)
            | (Type::F64, Type::Str)
            | (Type::BigInt, Type::Str)
            | (Type::Substr, Type::I64)
            | (Type::Substr, Type::F64)
            | (Type::Substr, Type::BigInt)
            | (Type::I64, Type::Substr)
            | (Type::F64, Type::Substr)
            | (Type::BigInt, Type::Substr)
    ) || (str_or_substr(a_ty) && bool_or_null(b_ty, &b))
        || (str_or_substr(b_ty) && bool_or_null(a_ty, &a))
        || (str_or_substr(a_ty) && arr_or_obj(b_ty))
        || (str_or_substr(b_ty) && arr_or_obj(a_ty))
        || (str_or_substr(a_ty) && fn_like(b_ty))
        || (str_or_substr(b_ty) && fn_like(a_ty))
        // Rotation 437 — Str + RegExp: §13.15.3 ToPrimitive →
        // §22.2.6.14 toString → "/source/flags". Without this pair
        // the operand fell through to the numeric arms and printed
        // the heap pointer as a number (measured on the checker-side
        // widening probe). An `any` side shares the kernel's walk.
        || (str_or_substr(a_ty) && (heap_cell_tostring(b_ty) || b_ty == Type::Any))
        || (str_or_substr(b_ty) && (heap_cell_tostring(a_ty) || a_ty == Type::Any))
        // A nullable STRING is a Str on both sides, so the plain
        // Str+Str concat below would have taken it and handed the
        // in-band 0 to `str_concat` — `"" + s` printed "undefined".
        // Route the pair through the coerce path instead, where the
        // sentinel guard lives.
        || (str_or_substr(a_ty) && str_or_substr(b_ty) && nullable_side);
    // Any Substr operand: route through view-aware concat
    // helpers. One alloc + two memcpys (vs. 2 allocs + 3
    // memcpys via substr_to_owned + str_concat).
    let either_substr = a_ty == Type::Substr || b_ty == Type::Substr;
    if either_substr
        && !nullable_side
        && (a_ty == Type::Str || a_ty == Type::Substr)
        && (b_ty == Type::Str || b_ty == Type::Substr)
    {
        let target = match (a_ty, b_ty) {
            (Type::Substr, Type::Str) => ctx.intrinsics.substr_concat_substr_str,
            (Type::Str, Type::Substr) => ctx.intrinsics.substr_concat_str_substr,
            (Type::Substr, Type::Substr) => ctx.intrinsics.substr_concat_substr_substr,
            _ => unreachable!(),
        };
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(target, vec![a, b]),
            Type::Str,
            None,
        );
        ctx.binop.str_check.emitted = true;
        drop_minted_temps(ctx, a, a_temp, b, b_temp);
        return Some(Operand::Value(v));
    }
    if a_ty == Type::Str && b_ty == Type::Str && !nullable_side {
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a, b]),
            Type::Str,
            None,
        );
        ctx.binop.str_check.emitted = true;
        drop_minted_temps(ctx, a, a_temp, b, b_temp);
        return Some(Operand::Value(v));
    }
    if mixed_string {
        let a_undefable = ctx.binop.left_f64_undefable;
        let b_undefable = ctx.binop.right_f64_undefable;
        // Cluster #6 (rotation 442) — a pointer-shaped nullable
        // operand is spelled as bare T here, so every arm of the
        // coerce below reads its in-band 0 as a live value: the Arr
        // arm handed NULL to `arr_join`, the Str arm passed the null
        // pointer through. Guard the sentinel first. A side this
        // lowering already replaced with its minted "undefined" holds
        // neither sentinel, and the guard's live arm would retain it a
        // second time.
        let (a_str, a_fresh) = if !a_temp && ctx.binop.left_nullable_ptr && ptr_shaped(&a_ty) {
            coerce_nullable_ptr_to_str(ctx, a)
        } else {
            coerce_to_str(ctx, a, a_undefable)
        };
        let (b_str, b_fresh) = if !b_temp && ctx.binop.right_nullable_ptr && ptr_shaped(&b_ty) {
            coerce_nullable_ptr_to_str(ctx, b)
        } else {
            coerce_to_str(ctx, b, b_undefable)
        };
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a_str, b_str]),
            Type::Str,
            None,
        );
        ctx.binop.str_check.emitted = true;
        drop_minted_temps(ctx, a_str, a_fresh || a_temp, b_str, b_fresh || b_temp);
        return Some(Operand::Value(v));
    }
    None
}

/// The heap cells whose ToString the any-lane kernel answers off the
/// header tag — one arm in `add_str_coerce`, this list, and
/// `check_add`'s. RegExp was the only one ever added, so the same
/// program with its `any` erased was a type error.
fn heap_cell_tostring(t: Type) -> bool {
    matches!(
        t,
        Type::RegExp
            | Type::Map
            | Type::Set
            | Type::WeakMap
            | Type::WeakSet
            | Type::WeakRef
            | Type::MapIter
            | Type::ArrIter
            | Type::Date
            | Type::Promise
    )
}

/// Release the fresh owned Str temps this lowering minted once the
/// concat has copied their bytes (the runtime helpers borrow their
/// operands). Non-minted operands are caller-owned — untouched.
fn drop_minted_temps(ctx: &mut LowerCtx, a: Operand, a_temp: bool, b: Operand, b_temp: bool) {
    if a_temp {
        ctx.emit_drop_value(a, Type::Str);
    }
    if b_temp {
        ctx.emit_drop_value(b, Type::Str);
    }
}
