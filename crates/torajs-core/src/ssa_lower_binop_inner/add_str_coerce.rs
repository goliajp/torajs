//! ToString for ONE operand of a mixed `+` concat — the arm set the
//! §13.15.3 dispatch in [`super::add_str`] hands each side to.
//!
//! The arm set is also what `ssa_lower_str_html`'s attribute-value
//! coercion reads — keep the two in sync.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260708-typed-arr-oob-read chunk 3 — ToString for a
/// possibly-sentinel F64: branch on the undefined-NaN sentinel bits
/// and answer the interned "undefined" literal vs `f64_to_str`.
/// Answers minted=true for both arms — dropping the STATIC_LITERAL
/// "undefined" is a no-op, the fresh `f64_to_str` Str needs it.
pub(crate) fn coerce_undefable_f64(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    use crate::ssa::{IPred, Terminator};
    let bits = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::BitCastF64ToI64(v), Type::I64, None);
    let is_undef = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(
            IPred::Eq,
            Operand::Value(bits),
            Operand::ConstI64(crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS as i64),
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

/// Which SSA shapes carry a pointer-shaped nullable's in-band 0.
/// Mirror of `ssa_lower_call_coercion_nullable::is_nullable_ptr`,
/// which asks the same question for the `String(x)` call face.
pub(super) fn ptr_shaped(t: &Type) -> bool {
    matches!(
        t,
        Type::Str | Type::Substr | Type::Arr(_) | Type::Obj(_) | Type::FnSig(_) | Type::Closure(_)
    )
}

/// Cluster #6 (rotation 442) — the pointer-shaped nullable operand's
/// coerce: branch on the in-band 0 sentinel (§13.15.3 ToString(null)
/// → "null" via the same `null_to_str` kernel the Str+Null pair uses)
/// before handing a live pointer to the plain coerce. The merge rides
/// an alloca'd Str slot (this IR has no phi), so every arm must store
/// an owned Str: the answer is always minted=true and the caller's one
/// drop balances whichever arm ran. A Str operand is borrowed, so its
/// live arm takes its own ref — storing it bare made that drop release
/// the caller's binding.
pub(super) fn coerce_nullable_ptr_to_str(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    let slot = ctx.alloca(Type::Str, None);
    let null_blk = ctx.f.add_block();
    let arr_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, v, Operand::ConstPtrNull),
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
    // The test above is against NULL, which is how `null` is spelled
    // in this slot — not how `undefined` is. A pointer-shaped slot
    // spells its undefined with the per-type immortal cell, and that
    // address rode the live-value arm straight into `coerce_to_str`,
    // which asked the cell for a `toString` it does not have: `const
    // o: { a: number } | undefined = undefined; "" + o` threw where
    // every engine prints "undefined" (§13.15.3 → ToString(undefined)).
    // Same bug the ToString call face had, in the operator's own
    // coercion.
    //
    // So split the live arm once more. Slot types with no sentinel
    // keep the two-way shape they had.
    if let Some(sentinel) = ctx.str_undef_sentinel_for(ctx.operand_ty(&v)) {
        let undef_blk = ctx.f.add_block();
        let live_blk = ctx.f.add_block();
        let is_undef = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, v, sentinel),
            Type::Bool,
            None,
        );
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: undef_blk,
                else_blk: live_blk,
            },
        );
        ctx.cur_block = undef_blk;
        let u = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.undefined_to_str, vec![]),
            Type::Str,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(u), Operand::Value(slot), 0),
        );
        ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
        ctx.cur_block = live_blk;
    }
    let (s, minted) = coerce_to_str(ctx, v, false);
    if !minted {
        ctx.emit_rc_inc(s);
    }
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
        Type::Ptr => {
            // V3-18 m1.d — ToString(null) is "null", for the literal
            // and for a binding of it alike (the guard here used to
            // admit only `ConstPtrNull`).
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
                Type::Str,
                None,
            );
            (Operand::Value(r), true)
        }
        // A fn side is a function OBJECT: ToPrimitive with the default
        // hint (valueOf first) over its canonical cell, the registry's
        // source row when none is bound. The closure side rides the
        // heap list below for the same walk.
        Type::FnSig(_) => (
            crate::ssa_lower_call_coercion_str::emit_fnsig_to_str(ctx, v, 0),
            true,
        ),
        // Rotation 437 — the RegExp side of a Str concat: the
        // §22.2.6.14 toString kernel ("/source/flags"), reached
        // through the any-lane heap dispatch the same way the Obj
        // arm below is (tag 4 = the Heap slot tag; the header tag
        // routes to the regex kernel).
        //
        // Every other heap cell rides the very same two instructions:
        // the kernel reads the header tag and answers §20.1.4.4's
        // "[object Map]" / "[object Promise]" / the Date string / the
        // two iterator names. It always could — `("x" as any) + m`
        // has printed all eight for as long as the kernel has existed
        // — so what refused `"x" + new Map()` was this list and the
        // checker's, not a missing answer. Symbol is deliberately
        // absent: §13.15.3 ToString of a Symbol is a TypeError, and
        // tsc refuses the pair too.
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
        // §13.15.3 — a closure side is a function OBJECT: ToPrimitive
        // (default hint) runs its own `valueOf` / `toString` before
        // the kernel's closure arm falls back to the source row. The
        // direct source kernel this arm used to call skipped both.
        | Type::Closure(_) => heap_cell_to_str(ctx, v),
        // S138 — the Obj side below runs OrdinaryToPrimitive; the Arr
        // side is the S137 dispatch, shared with the two `String` /
        // `Number` coercion sites.
        Type::Arr(elem_arr_id) => {
            crate::ssa_lower_coerce_arr::emit_concat_side(ctx, v, elem_arr_id)
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
        Type::Obj(_) => heap_cell_to_str(ctx, v),
        // §13.15.3 — ToPrimitive(default) then ToString of whatever
        // the box holds
        Type::Any => any_to_str(ctx, v),
        other => panic!("ssa-lower: mixed string concat unexpected type {other:?}"),
    }
}

/// A heap cell's §13.15.3 ToString through the any-lane kernel, which
/// runs ToPrimitive(default) and dispatches on the header tag (tag 4 is
/// the Heap slot tag).
fn heap_cell_to_str(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    let raw = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::PtrToInt(v), Type::I64, None);
    prim_pair_to_str(ctx, Operand::ConstI64(4), Operand::Value(raw), None)
}

/// An `any` side: the kernel borrows the unboxed pair, which is settled
/// back into the box once the Str is built.
fn any_to_str(ctx: &mut LowerCtx, v: Operand) -> (Operand, bool) {
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![v]),
        Type::I64,
        None,
    );
    let raw = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_unbox_value, vec![v]),
        Type::I64,
        None,
    );
    prim_pair_to_str(ctx, Operand::Value(tag), Operand::Value(raw), Some(v))
}

fn prim_pair_to_str(
    ctx: &mut LowerCtx,
    tag: Operand,
    raw: Operand,
    settle: Option<Operand>,
) -> (Operand, bool) {
    let s = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_to_str_prim, vec![tag, raw]),
        Type::Str,
        None,
    );
    if let Some(src) = settle {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_settle, vec![src, raw]),
        );
    }
    ctx.emit_throw_check(None);
    (Operand::Value(s), true)
}
