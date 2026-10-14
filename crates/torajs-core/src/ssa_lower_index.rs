//! `Expr::Index { obj, index }` lowering pulled out of
//! [`crate::ssa_lower::lower_expr_inner`]'s match arm as chunk-67
//! of the decomp (chunks 1-66 = ... + P8.2 accessor + struct-
//! field terminal arm).
//!
//! Lowers `xs[i]` for three receiver shapes:
//!
//! - **String indexing** (`Type::Str` / `Type::Substr`) — returns a
//!   single-char `Type::Substr` view.
//!   - **V3-18 m1.h.44** — Str routes through
//!     `__torajs_str_char_at(s, idx)` for bounds-checked indexing
//!     (direct `substr_create` trusted user idx and OOB stored
//!     garbage offsets; same fix as charAt m1.h.37).
//!   - Substr routes through
//!     `__torajs_substr_slice(v, idx, idx+1)` (resolves to root
//!     parent via runtime).
//! - **Array<Any> indexed read** (`Type::Arr(arr_id)` with
//!   `elem_ty == Type::Any`) — 16-byte slot stride dual-load
//!   `arr_get_any_tag` + `arr_get_any_value` + `any_box` packs
//!   the (tag, value) pair into a single Any-box ptr the SSA
//!   layer can carry. Per-read alloc is the trade-off vs SSA-
//!   layer pair passing; use-site fast paths (`console.log(xs[i])`)
//!   may inline direct dispatch without box (T-10.e). **P1.4**
//!   bounds-check is in the runtime helper: OOB returns
//!   `ANY_UNDEF=5` per ES spec §10.4.2.1 (pre-P1.4 inlined the
//!   `LoadDyn` at `24 + (head+i)*16` unconditionally and returned
//!   garbage / ANY_NULL).
//! - **Array<T>** (`Type::Arr(arr_id)` with concrete `elem_ty`) —
//!   `LoadDyn(elem_ty, arr_val, offset)` where offset is computed
//!   by `emit_arr_slot_byte_offset` (T-13.5 deque: `24 + (idx +
//!   head) * 8`; non-deque names take the fast path via 11-A1
//!   `arr_expr_is_non_deque` peek).
//!   - **All concrete elems** are bounds-branched (RFC
//!     20260708-typed-arr-oob-read chunks 1-2): OOB answers the
//!     immortal Str sentinel (`Str`), the undefined-NaN sentinel
//!     bits (`F64`), or a catchable RangeError (I64 / Bool /
//!     nested heap — no `undefined` representation in the slot).
//!     See `lower_typed_index_checked`.
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Index` match arm bottoms out here, no None fall-through).

pub(crate) mod dict;

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) use crate::ssa_lower_index_recv_shape::{shape_has_expando_bag, to_object_needs_boxing};

pub(crate) use crate::ssa_lower_index_any_key::{
    lower_any_index_str_key, lower_any_index_symbol_key, lower_any_key, lower_array_any_index,
    lower_struct_like_index, lower_typed_arr_any_key,
};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId, index: ExprId) -> Operand {
    if let Some(op) = crate::ssa_lower_exec_record::try_index(ctx, eid) {
        return op;
    }
    // Chunk 745 — struct receiver + compile-time literal index:
    // `g[0]` ≡ `g."0"` per ES ToPropertyKey (§7.1.19); the full
    // member-read dispatcher handles the field load (struct layout /
    // accessor / owned-receiver release). Same gate as the checker
    // lane in `check_type_of_index` (un-resolved Struct only), so
    // checker-admitted struct indices never fall through to the
    // array panic below.
    if obj_is_struct_like(ctx, obj)
        && let Some(key) = crate::ast::literal_prop_key(ctx.ast, index)
        && let Some(name) = key.as_str()
    {
        return crate::ssa_lower_member::lower(ctx, eid, obj, name);
    }
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    lower_from_value(ctx, eid, obj, index, arr_val, is_non_deque)
}

/// Value-based body: the receiver is already lowered (`arr_val`),
/// `obj` is only consulted for side tables (nullable-guard set /
/// bounds-proven windows). Chunk 703 — `obj?.[index]`'s hit path
/// enters here so the receiver evaluates exactly once and the index
/// expression lowers inside the hit block (ES short-circuit: a
/// nullish receiver never evaluates `index`). `eid` is the consumer-
/// visible expression id (Index or OptIndex) — the literal-string-key
/// lane records it as an owned read (chunk 717).
pub(crate) fn lower_from_value(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    index: ExprId,
    arr_val: Operand,
    is_non_deque: bool,
) -> Operand {
    let arr_ty = ctx.operand_ty(&arr_val);
    if matches!(arr_ty, Type::Str | Type::Substr) {
        return crate::ssa_lower_index_any_key::lower_str_recv_index(
            ctx, eid, obj, arr_val, arr_ty, index,
        );
    }
    // Chunk 753 — struct receiver + dynamic index: encode the struct
    // cell as a NaN-box (`box_to_any`'s refcounted arm is a pure
    // pointer ENCODING — zero rc traffic, the receiver binding keeps
    // its stake and the box is a borrow, so no release here; an
    // eager drop dec'd the receiver per read and freed it under
    // later allocs) and ride the any-index runtime lane — the
    // struct-box probe path answers fields as owned boxes, a miss
    // answers undefined. Literal keys normally take the callers'
    // chunk-745 member gate; OptIndex's hit path lands them here
    // instead and the same runtime lane answers them. The checker
    // admits the dynamic form with an Any answer; dynamic writes
    // keep the loud reject.
    if matches!(arr_ty, Type::Obj(_)) && obj_is_struct_like(ctx, obj) {
        return lower_struct_like_index(ctx, eid, index, arr_val);
    }
    // RFC 20260802-class-computed-member 刀 3a — a STRUCT receiver's
    // keyed read rides the any lane: the box is a pure tag-4 encode,
    // and member_get's struct arm carries the class-layout field
    // probe plus the class prototype chain, so both an own field
    // spelled dynamically (`c["m"]`) and a runtime-computed member
    // (`c[Symbol.iterator]`) resolve. Numeric keys land the same
    // keyed kernels' ToPropertyKey spelling. A REGEXP receiver rides
    // the same box (r289): `re[Symbol.match]` is a prototype-surface
    // property read, and the checker only admits its non-number keys.
    // A FUNCTION receiver boxes too — its numeric read is the
    // stringified expando probe on the closure cell (checker admit
    // in check_type_of_index; write mirror in the assign lowering).
    let obj_is_fn = matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Function(..))
    );
    // A MAP or SET receiver joins the same box (rotation 609). The
    // checker has answered `Any` for their non-number keys since
    // r289 — `m[Symbol.iterator]` is an ordinary prototype-surface
    // read — but this list never learned them, so every one of those
    // keys, the number included, died "index access on non-array
    // type Map". Boxing is the whole mechanism: RegExp has been on
    // this list the whole time and its non-number keys have worked
    // the whole time.
    // r642 — a Closure OPERAND under an `Any` type (`(f as any)[0]`) boxes too.
    let (arr_val, arr_ty) = if shape_has_expando_bag(&arr_ty)
        || obj_is_fn
        || matches!(arr_ty, Type::Closure(_) | Type::FnSig(_))
        || crate::ssa_lower_index_recv_shape::primitive_index_receiver(&arr_ty)
    {
        (ctx.box_to_any(arr_val), Type::Any)
    } else {
        (arr_val, arr_ty)
    };
    // Any-dynamic-access RFC (20260704) S3 — `recv[i]` where recv is
    // an `any` value: runtime dispatch (kind-aware Arr / Str /
    // primitive) via `__torajs_any_index_get`. A null/undefined
    // receiver records a pending catchable TypeError, so the throw
    // check follows the call.
    if arr_ty == Type::Any {
        // L3b #13 (chunk 528) — string keys probe properties per ES
        // ToPropertyKey. A compile-time literal rides the full
        // member-read path (`o["k"]` ≡ `o.k`: class IC / length /
        // regexp props / probe fallback); a dynamic string key
        // probes by its runtime Str cell (a dynamic key that names
        // "length" lands the own-property probe — recorded
        // boundary).
        if let crate::ast::Expr::String(lit) = ctx.ast.get_expr(index) {
            let lit = lit.clone();
            return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, arr_val, &lit);
        }
        if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::String)) {
            // Chunk 726 convention (OptIndex already does this) —
            // owned track + release an owned-shape receiver temp,
            // mirroring the numeric lane below.
            let number_dict = ctx.ast.number_dicts.contains(&obj);
            let v = lower_any_index_str_key(ctx, arr_val, index, number_dict);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            return v;
        }
        // §6.1.7 / §7.1.19 step 2 — a symbol key reaches the probe as
        // its own cell, uncoerced. Same owned-read bookkeeping as the
        // string-key arm; the probe borrows the key, so an Ident key
        // keeps its stake and its scope drop.
        if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::Symbol)) {
            let v = lower_any_index_symbol_key(ctx, arr_val, index);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            return v;
        }
        // Cluster #1 blade 3 — an `any`-typed KEY can't pick a static
        // lane (number / string / symbol all possible at runtime);
        // the keyed kernel does the §7.1.19 ToPropertyKey dispatch.
        // Probe borrows the key; same owned-read bookkeeping as the
        // numeric lane below. An UNDEFINED key joins (rotation 346);
        // a STRUCT key joins (cluster #4, boxed by `lower_any_key`).
        if ctx
            .expr_types
            .get(&index)
            .is_some_and(crate::check_type_of_index::key_rides_keyed_kernel)
        {
            // Cluster #4 T4 — reuse a compound write lane's pinned
            // coerced key (see the boxed-struct twin above).
            let (k_raw, memoed) = match &ctx.pins.compound_key {
                Some((mid, mop)) if *mid == index => (*mop, true),
                _ => (lower_any_key(ctx, index), false),
            };
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.any_index_get_keyed, vec![arr_val, k_raw]),
                Type::Any,
                None,
            );
            ctx.emit_throw_check(None);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            if !memoed && ctx.expr_transfers_ownership(index) {
                ctx.emit_drop_value(k_raw, Type::Any);
            }
            return Operand::Value(v);
        }
        let v =
            crate::ssa_lower_index_any_key::AnyNumericKey::lower(ctx, index).emit_get(ctx, arr_val);
        ctx.emit_throw_check(None);
        // Kernel cells answer +1 (`__torajs_arr_index_get` doc) —
        // record the read as owned so consumers release it, and
        // release an owned-shape receiver temp (`o.a[0]` /
        // `f()[0]`: the any-member read / call result had no other
        // consumer). Pre-record both stakes stranded (~128B/iter
        // element + ~64B/iter receiver on the churn probes).
        ctx.owned_member_reads.insert(eid);
        ctx.release_owned_temp(obj, &arr_val);
        return Operand::Value(v);
    }
    // A receiver that is an owned temp (`f()[0]`, `s.split(",")[0]`,
    // `[a, b][i]`) has no consumer but this read: the element takes
    // its own stake and the receiver is released here, as the member
    // lane does for `f().x`. The stake is parked across the lanes'
    // throw edges (nullable guard, index conversion, OOB RangeError).
    let parked = ctx.park_owned_temp(obj, &arr_val);
    let elem = crate::ssa_lower_index_owned_recv::lower_typed_arr(
        ctx,
        eid,
        obj,
        index,
        arr_val,
        arr_ty,
        is_non_deque,
    );
    ctx.unpark_owned_temp(parked);
    crate::ssa_lower_index_owned_recv::detach_from_owned_receiver(ctx, eid, obj, &arr_val, elem)
}

/// Typed-array indexed read with the OOB bounds branch (RFC
/// 20260708-typed-arr-oob-read chunks 1-2). A negative or `>= len`
/// index takes a per-elem-type `undefined` exit instead of reading
/// a garbage/NULL slot:
///
/// - `Str` — the immortal `undefined` Str sentinel (chunk 1); every
///   consumer rides the chunk 649-667 family, and the sentinel's
///   FLAG_STATIC_LITERAL keeps the borrow-shaped read convention.
/// - `F64` — the undefined-NaN sentinel bits (chunk 2). Handing it
///   to arithmetic is ToNumber(undefined) = NaN, and the machine
///   performs that step itself: the program entry selects FPCR.DN,
///   so an FP operation cannot carry the payload out
///   (`torajs-cli::cmd_build_synthesize::FPCR_DN_BIT`). typeof /
///   strict-eq / nullish / print / box consumers gate STATICALLY on
///   `is_undef_f64_source` and re-check the bits at runtime.
/// - everything else (I64 / Bool / nested heap) — no `undefined`
///   representation in the slot type: `__torajs_arr_oob_throw`
///   records a catchable RangeError (loud, never garbage) and the
///   merge tail propagates it via the throw check.
///
/// In-bounds reads keep the direct LoadDyn. `upper_proven` drops the
/// `>= len` compare and the length load it needs, for a read an
/// enclosing `i < arr.length` guard settles — the negative compare
/// stays, because that guard says nothing about `i >= 0` and a
/// negative index reads before the data pointer. `oob_asks` sends the
/// out-of-range exit to the prototype chain's property
/// ([`crate::ssa_lower_index_nonintegral::emit_oob_named_read`]); a
/// plain `[[Get]]` passes the program's `array_prop_reads_may_raise`.
#[allow(clippy::too_many_arguments)] // two call sites, each argument an independent switch of one read
pub(crate) fn lower_typed_index_checked(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
    is_non_deque: bool,
    elem_ty: Type,
    upper_proven: bool,
    nonint: Option<(Operand, Operand)>,
    oob_asks: bool,
) -> Operand {
    use crate::ssa::{IPred, Terminator};
    use crate::ssa_lower::ARR_LEN_OFF;
    let len = (!upper_proven).then(|| {
        ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, arr_val, ARR_LEN_OFF),
            Type::I64,
            None,
        )
    });
    let slot_ty = crate::ssa_lower_nullable_guard::undefable_read_ty(elem_ty);
    let promote_bool = slot_ty != elem_ty;
    let owned_str = elem_ty == Type::Str && (nonint.is_some() || oob_asks);
    let slot = ctx.alloca_in_entry(slot_ty, Some("__oob_elem"));
    let oob_blk = ctx.f.add_block();
    let in_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    // A non-integral index names a property, not an element
    // (`lower_typed_read_index`): an expando the store side may have
    // made, read through the keyed kernel into the slot shape where
    // the slot can spell undefined; otherwise the RangeError exit.
    if let Some((nonint, raw)) = nonint {
        let int_blk = ctx.f.add_block();
        let prop_blk = ctx.f.add_block();
        let cb = ctx.cur_block;
        ctx.f.set_term(
            cb,
            Terminator::CondBr {
                cond: nonint,
                then_blk: prop_blk,
                else_blk: int_blk,
            },
        );
        ctx.cur_block = prop_blk;
        if crate::ssa_lower_index_nonintegral::emit_nonintegral_typed_read(
            ctx, arr_val, raw, elem_ty, slot,
        ) {
            let pb = ctx.cur_block;
            ctx.f.set_term(pb, Terminator::Br(merge));
        } else {
            ctx.f.set_term(prop_blk, Terminator::Br(oob_blk));
        }
        ctx.cur_block = int_blk;
    }
    // one unsigned compare covers both ends (len never exceeds
    // i64::MAX); a proven upper bound leaves only the sign test
    let out = match len {
        Some(len) => ctx.cmp(IPred::Uge, idx_val, Operand::Value(len)),
        None => ctx.cmp(IPred::Slt, idx_val, Operand::ConstI64(0)),
    };
    let cb = ctx.cur_block;
    let oob_target = if oob_asks {
        crate::ssa_lower_index_nonintegral::emit_oob_named_read(
            ctx, arr_val, idx_val, elem_ty, slot, merge,
        )
        .unwrap_or(oob_blk)
    } else {
        oob_blk
    };
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: out,
            then_blk: oob_target,
            else_blk: in_blk,
        },
    );
    let (oob_val, oob_throws) =
        crate::ssa_lower_index_undef::oob_undef_value(ctx, oob_blk, elem_ty);
    ctx.f
        .append_void(oob_blk, InstKind::Store(oob_val, Operand::Value(slot), 0));
    ctx.f.set_term(oob_blk, Terminator::Br(merge));
    ctx.cur_block = in_blk;
    let (offset_base, offset) = ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 3, is_non_deque);
    // A boolean element loads as its raw word: a hole is the word 2,
    // which the check below has to see before it becomes a boolean.
    let load_ty = if promote_bool { Type::I64 } else { elem_ty };
    let cur = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur,
        InstKind::LoadDyn(load_ty, offset_base, offset),
        load_ty,
        None,
    );
    crate::ssa_lower_index_undef::hole_slot_is_undefined(ctx, v, elem_ty, slot, merge);
    // the getter exits answer an owned Str, so the element joins them
    // owned (`lower_typed_arr` records the read)
    if owned_str {
        ctx.emit_rc_inc(Operand::Value(v));
    }
    let stored = if promote_bool {
        let b = ctx.cmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0));
        ctx.box_to_any(b)
    } else {
        Operand::Value(v)
    };
    let cur = ctx.cur_block;
    ctx.f
        .append_void(cur, InstKind::Store(stored, Operand::Value(slot), 0));
    ctx.f.set_term(cur, Terminator::Br(merge));
    ctx.cur_block = merge;
    let out = ctx.f.append_inst(
        merge,
        InstKind::Load(slot_ty, Operand::Value(slot), 0),
        slot_ty,
        None,
    );
    if oob_throws {
        // propagate the RangeError recorded on the oob path.
        ctx.emit_throw_check(None);
    }
    Operand::Value(out)
}

/// The receiver shapes `check_type_of_index` admits: a bare struct, and
/// — since RFC 20260715-nominal-class-identity — a class instance, whose
/// checked type now NAMES its class instead of collapsing to the field
/// struct behind it. Both index the same way (`this["#m"]` reads the
/// field; a dynamic key rides the any-index lane), so both gates below
/// have to see them alike, or a class receiver falls through to the
/// array panic.
fn obj_is_struct_like(ctx: &LowerCtx<'_>, obj: crate::ast::ExprId) -> bool {
    matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Struct(_) | crate::check::Type::ClassRef(_))
    )
}
