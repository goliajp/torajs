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
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

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
    let bool_or_null = |t: Type, op: &Operand| -> bool {
        matches!(t, Type::Bool) || matches!(op, Operand::ConstPtrNull)
    };
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
        // widening probe).
        || (str_or_substr(a_ty) && b_ty == Type::RegExp)
        || (str_or_substr(b_ty) && a_ty == Type::RegExp);
    // Any Substr operand: route through view-aware concat
    // helpers. One alloc + two memcpys (vs. 2 allocs + 3
    // memcpys via substr_to_owned + str_concat).
    let either_substr = a_ty == Type::Substr || b_ty == Type::Substr;
    if either_substr
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
            InstKind::Call(target, vec![a.clone(), b.clone()]),
            Type::Str,
            None,
        );
        drop_minted_temps(ctx, a, a_temp, b, b_temp);
        return Some(Operand::Value(v));
    }
    if a_ty == Type::Str && b_ty == Type::Str {
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a.clone(), b.clone()]),
            Type::Str,
            None,
        );
        drop_minted_temps(ctx, a, a_temp, b, b_temp);
        return Some(Operand::Value(v));
    }
    if mixed_string {
        let a_undefable = ctx.binop.left_f64_undefable;
        let b_undefable = ctx.binop.right_f64_undefable;
        // Cluster #6 (rotation 442) — a nullable-arr operand (an
        // un-narrowed `match`/`exec` result; SSA repr a plain Arr
        // pointer with the in-band 0 sentinel) guards the sentinel
        // before the Arr coerce would hand NULL to `arr_join`.
        let (a_str, a_fresh) = if ctx.binop.left_nullable_arr && matches!(a_ty, Type::Arr(_)) {
            coerce_nullable_arr_to_str(ctx, a)
        } else {
            coerce_to_str(ctx, a, a_undefable)
        };
        let (b_str, b_fresh) = if ctx.binop.right_nullable_arr && matches!(b_ty, Type::Arr(_)) {
            coerce_nullable_arr_to_str(ctx, b)
        } else {
            coerce_to_str(ctx, b, b_undefable)
        };
        let concat = ctx.intrinsics.str_concat;
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(concat, vec![a_str.clone(), b_str.clone()]),
            Type::Str,
            None,
        );
        drop_minted_temps(ctx, a_str, a_fresh || a_temp, b_str, b_fresh || b_temp);
        return Some(Operand::Value(v));
    }
    None
}

/// RFC 20260708-typed-arr-oob-read chunk 3 — ToString for a
/// possibly-sentinel F64: branch on the undefined-NaN sentinel bits
/// and answer the interned "undefined" literal vs `f64_to_str`.
/// Answers minted=true for both arms — dropping the STATIC_LITERAL
/// "undefined" is a no-op, the fresh `f64_to_str` Str needs it.
pub(crate) fn coerce_undefable_f64(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    use crate::ssa::{IPred, Terminator};
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BitCastF64ToI64(v.clone()),
        Type::I64,
        None,
    );
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(bits),
            Operand::ConstI64(crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS as i64),
        ),
        Type::Bool,
        None,
    );
    let undef_blk = ctx.f.add_block();
    let num_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let slot = ctx.alloca_in_entry(Type::Str, Some("__custr"));
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: Operand::Value(is_undef),
            then_blk: undef_blk,
            else_blk: num_blk,
        },
    );
    ctx.cur_block = undef_blk;
    let lit = ctx.intern_string_literal("undefined");
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lit), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = num_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.f64_to_str, vec![v]),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(r), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let out = ctx.f.append_inst(
        merge,
        InstKind::Load(Type::Str, Operand::Value(slot), 0),
        Type::Str,
        None,
    );
    (Operand::Value(out), true)
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

/// Cluster #6 (rotation 442) — the nullable-arr operand's coerce:
/// branch on the in-band 0 sentinel (§13.15.3 ToString(null) →
/// "null" via the same `null_to_str` kernel the Str+Null pair uses)
/// before handing a live pointer to the plain Arr coerce
/// (`arr_join`). The merge rides an alloca'd Str slot (this IR has
/// no phi). Both arms mint a fresh Str, so the answer is always
/// minted=true and the caller's temp drop balances either path.
fn coerce_nullable_arr_to_str(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    let slot = ctx.alloca(Type::Str, None);
    let null_blk = ctx.f.add_block();
    let arr_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v.clone(), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: null_blk,
            else_blk: arr_blk,
        },
    );
    ctx.cur_block = null_blk;
    let n = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
        Type::Str,
        None,
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(n), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = arr_blk;
    let (s, _minted) = coerce_to_str(ctx, v, false);
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(s, Operand::Value(slot), 0));
    ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
    ctx.cur_block = merge;
    let out = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(slot), 0),
        Type::Str,
        None,
    );
    (Operand::Value(out), true)
}

/// One operand → owned Str for the mixed-concat path (body is the
/// pre-split `coerce` closure, verbatim). The bool is true when the
/// Str was minted here (fresh owned — caller drops it post-concat);
/// pass-throughs and interned literals answer false.
///
/// `undefable` — RFC 20260708-typed-arr-oob-read chunk 3: the F64
/// side may hold the undefined-NaN sentinel (number[] index read /
/// alias, per the caller's `binop_*_f64_undefable` flag); its arm
/// branches on the bits and answers "undefined" instead of "NaN"
/// (covers `\`${a[i]}\`` templates — the parser desugars them to
/// this concat chain).
/// Shared with `ssa_lower_str_html`'s attribute-value coercion —
/// keep the arm set in sync with the mixed-concat surface.
pub(crate) fn coerce_to_str(ctx: &mut LowerCtx, v: Operand, undefable: bool) -> (Operand, bool) {
    match ctx.operand_ty(&v) {
        Type::Str => (v, false),
        Type::Substr => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::I64 => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.i64_to_str, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::F64 => {
            if undefable {
                return coerce_undefable_f64(ctx, v);
            }
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.f64_to_str, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::Bool => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.bool_to_str, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::BigInt => {
            // V3-18 m3.c — BigInt → String concat. The
            // BigInt is consumed by bigint_to_string
            // (rc-managed; helper handles the inc).
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.bigint_to_string, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::Ptr if matches!(v, Operand::ConstPtrNull) => {
            // V3-18 m1.d — null literal → "null".
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        // RFC 20260719-fn-tostring-source B5 — fn side of a Str
        // concat: the erased-source toString kernels (mirror of the
        // String() lane's emit_to_string arms).
        Type::FnSig(_) => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.fn_source_str, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        Type::Closure(_) => {
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.closure_source_str, vec![v]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        // Rotation 437 — the RegExp side of a Str concat: the
        // §22.2.6.14 toString kernel ("/source/flags"), reached
        // through the any-lane heap dispatch the same way the Obj
        // arm below is (tag 4 = the Heap slot tag; the header tag
        // routes to the regex kernel).
        Type::RegExp => {
            let raw = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::PtrToInt(v.clone()),
                Type::I64,
                None,
            );
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_to_str_prim,
                    vec![Operand::ConstI64(4), Operand::Value(raw)],
                ),
                Type::Str,
                None,
            );
            (Operand::Value(s), true)
        }
        // S138 — Arr / Obj sides reuse the S137 dispatch.
        Type::Arr(elem_arr_id) => {
            let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
            let join_fid = match elem_ty {
                Type::Substr => ctx.intrinsics.arr_join_substr,
                Type::I64 => ctx.intrinsics.arr_join_i64,
                Type::F64 => ctx.intrinsics.arr_join_f64,
                Type::Bool => ctx.intrinsics.arr_join_bool,
                Type::Any => ctx.intrinsics.arr_join_any,
                _ => ctx.intrinsics.arr_join,
            };
            let sep = ctx.intern_string_literal(",");
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(join_fid, vec![v, Operand::Value(sep)]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        // An object side runs OrdinaryToPrimitive at runtime (RFC
        // 20260712 chunk C — mirror of the String(struct) S137 emit),
        // which also answers the §20.1.4.4 literal through
        // Object.prototype.toString when the receiver has no hook.
        //
        // This used to shortcut to that literal statically whenever the
        // LAYOUT carried no `toString` / `valueOf` field. A class
        // instance shares the `Type::Obj` slot and keeps its methods on
        // the prototype, never in the layout, so the test answered no
        // for every class — `"x" + c` would have printed
        // "[object Object]" over a user `toString`.
        Type::Obj(_) => {
            let raw = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::PtrToInt(v.clone()),
                Type::I64,
                None,
            );
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_to_str_prim,
                    vec![Operand::ConstI64(4), Operand::Value(raw)],
                ),
                Type::Str,
                None,
            );
            ctx.emit_throw_check(None);
            (Operand::Value(s), true)
        }
        other => panic!("ssa-lower: mixed string concat unexpected type {other:?}"),
    }
}
