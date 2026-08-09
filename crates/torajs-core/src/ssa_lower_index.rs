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

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) use crate::ssa_lower_index_any_key::{
    lower_any_index_str_key, lower_any_index_symbol_key, lower_any_key, lower_array_any_index,
    lower_typed_arr_any_key,
};

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId, index: ExprId) -> Operand {
    // Chunk 745 — struct receiver + compile-time literal index:
    // `g[0]` ≡ `g."0"` per ES ToPropertyKey (§7.1.19); the full
    // member-read dispatcher handles the field load (struct layout /
    // accessor / owned-receiver release). Same gate as the checker
    // lane in `check_type_of_index` (un-resolved Struct only), so
    // checker-admitted struct indices never fall through to the
    // array panic below.
    if obj_is_struct_like(ctx, obj)
        && let Some(name) = crate::ast::literal_prop_key(ctx.ast, index)
    {
        return crate::ssa_lower_member::lower(ctx, eid, obj, &name);
    }
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let arr_val = ctx.lower_expr(obj);
    lower_from_value(ctx, eid, obj, index, arr_val, is_non_deque)
}

/// The struct-like receiver's dynamic-index read (chunk 753; carved
/// out of [`lower_from_value`] when the rotation-346 undefined-key
/// arm pushed it past the 200-line fn limit — body verbatim). The
/// struct cell encodes as a NaN-box (pure pointer encoding, zero rc
/// traffic — the receiver binding keeps its stake and the box is a
/// borrow) and every key shape rides the any-index runtime lane.
fn lower_struct_like_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    index: ExprId,
    arr_val: Operand,
) -> Operand {
    let boxed = ctx.box_to_any(arr_val);
    if let crate::ast::Expr::String(lit) = ctx.ast.get_expr(index) {
        let lit = lit.clone();
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, boxed, &lit);
    }
    if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::String)) {
        // Chunk 726 convention (OptIndex already does this) —
        // the dynamic-key probe answers a fresh owned box; the
        // eid joins the owned track so the consumer releases.
        let v = lower_any_index_str_key(ctx, boxed, index);
        ctx.owned_member_reads.insert(eid);
        return v;
    }
    // Cluster #1 blade 5 — an `any`-typed key on the boxed struct
    // rides the keyed kernel's ToPropertyKey dispatch (mirror of
    // the Any-receiver arm below). An UNDEFINED key joins
    // (rotation 346): `lower_any_key` boxes it to ANY_UNDEF.
    if matches!(
        ctx.expr_types.get(&index),
        Some(crate::check::Type::Any | crate::check::Type::Undefined)
    ) {
        let k_raw = lower_any_key(ctx, index);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(
                ctx.intrinsics.any_index_get_keyed,
                vec![boxed, k_raw.clone()],
            ),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        ctx.owned_member_reads.insert(eid);
        if ctx.expr_transfers_ownership(index) {
            ctx.emit_drop_value(k_raw, Type::Any);
        }
        return Operand::Value(v);
    }
    let idx_val = ctx.lower_index_operand(index);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_index_get, vec![boxed, idx_val]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    // `__torajs_arr_index_get` answers cells with an explicit +1
    // ("the returned copy takes another") — record the read as
    // owned so the consumer releases it (chunk 717 convention);
    // pre-record the +1 was stranded (~128B/iter on the
    // any-index churn probe). The boxed receiver itself is a
    // borrow (chunk 753 pure-encoding note above), no release.
    ctx.owned_member_reads.insert(eid);
    return Operand::Value(v);
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
            ctx, eid, arr_val, arr_ty, index,
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
    let (arr_val, arr_ty) = if matches!(arr_ty, Type::Obj(_) | Type::RegExp) {
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
            let v = lower_any_index_str_key(ctx, arr_val.clone(), index);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            return v;
        }
        // §6.1.7 / §7.1.19 step 2 — a symbol key reaches the probe as
        // its own cell, uncoerced. Same owned-read bookkeeping as the
        // string-key arm; the probe borrows the key, so an Ident key
        // keeps its stake and its scope drop.
        if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::Symbol)) {
            let v = lower_any_index_symbol_key(ctx, arr_val.clone(), index);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            return v;
        }
        // Cluster #1 blade 3 — an `any`-typed KEY can't pick a static
        // lane (number / string / symbol all possible at runtime);
        // the keyed kernel does the §7.1.19 ToPropertyKey dispatch.
        // Probe borrows the key; same owned-read bookkeeping as the
        // numeric lane below. An UNDEFINED key joins (rotation 346).
        if matches!(
            ctx.expr_types.get(&index),
            Some(crate::check::Type::Any | crate::check::Type::Undefined)
        ) {
            let k_raw = lower_any_key(ctx, index);
            let cur_block = ctx.cur_block;
            let v = ctx.f.append_inst(
                cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_index_get_keyed,
                    vec![arr_val.clone(), k_raw.clone()],
                ),
                Type::Any,
                None,
            );
            ctx.emit_throw_check(None);
            ctx.owned_member_reads.insert(eid);
            ctx.release_owned_temp(obj, &arr_val);
            if ctx.expr_transfers_ownership(index) {
                ctx.emit_drop_value(k_raw, Type::Any);
            }
            return Operand::Value(v);
        }
        let idx_val = ctx.lower_index_operand(index);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_index_get, vec![arr_val.clone(), idx_val]),
            Type::Any,
            None,
        );
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
    if matches!(arr_ty, Type::Arr(_))
        && matches!(
            ctx.expr_types.get(&index),
            Some(crate::check::Type::Any | crate::check::Type::String | crate::check::Type::Symbol)
        )
    {
        return lower_typed_arr_any_key(ctx, eid, arr_val, index);
    }
    // RC-4 F1a — a nullable-arr receiver (exec/match result) may be
    // null on miss; guard before the element load (both Arr<Any>
    // and typed Arr<T> lanes).
    crate::ssa_lower_nullable_guard::emit_nullable_arr_guard(ctx, obj, &arr_val);
    let elem_ty = match arr_ty {
        Type::Arr(arr_id) => ctx.arr_layouts[arr_id.0 as usize],
        other => panic!("ssa-lower: index access on non-array type {other:?}"),
    };
    let idx_val = ctx.lower_index_operand(index);
    if elem_ty == Type::Any {
        return lower_array_any_index(ctx, arr_val, idx_val);
    }
    // Guard-dominated bounds elision — `xs[i]` under an enclosing
    // `i < xs.length` guard (untainted window) keeps the direct
    // load; see [`crate::ssa_lower_bounds_proven`].
    if crate::ssa_lower_bounds_proven::is_proven(ctx, obj, index) {
        let (offset_base, offset) =
            ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 3, is_non_deque);
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::LoadDyn(elem_ty, offset_base, offset),
            elem_ty,
            None,
        );
        return Operand::Value(v);
    }
    lower_typed_index_checked(ctx, arr_val, idx_val, is_non_deque, elem_ty)
}

/// Typed-array indexed read with the OOB bounds branch (RFC
/// 20260708-typed-arr-oob-read chunks 1-2). A negative or `>= len`
/// index takes a per-elem-type `undefined` exit instead of reading
/// a garbage/NULL slot:
///
/// - `Str` — the immortal `undefined` Str sentinel (chunk 1); every
///   consumer rides the chunk 649-667 family, and the sentinel's
///   FLAG_STATIC_LITERAL keeps the borrow-shaped read convention.
/// - `F64` — the undefined-NaN sentinel bits (chunk 2); arithmetic
///   propagates it as a plain NaN, while typeof / strict-eq /
///   nullish / print / box consumers gate STATICALLY on
///   `is_undef_f64_source` and re-check the bits at runtime.
/// - everything else (I64 / Bool / nested heap) — no `undefined`
///   representation in the slot type: `__torajs_arr_oob_throw`
///   records a catchable RangeError (loud, never garbage) and the
///   merge tail propagates it via the throw check.
///
/// In-bounds reads keep the direct LoadDyn; the two dominated
/// compares are loop-eliminable under `i < arr.length` guards.
pub(crate) fn lower_typed_index_checked(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
    is_non_deque: bool,
    elem_ty: Type,
) -> Operand {
    use crate::ssa::{IPred, Terminator};
    use crate::ssa_lower::ARR_LEN_OFF;
    use crate::ssa_lower_binop_null_undef::UNDEF_CELL_SYM;
    use crate::ssa_lower_intrinsics_str_b::STR_UNDEF_CELL_SYM;
    use crate::ssa_lower_nullable_guard::F64_UNDEF_SENTINEL_BITS;
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, arr_val.clone(), ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let slot_ty = crate::ssa_lower_nullable_guard::undefable_read_ty(elem_ty);
    let promote_bool = slot_ty != elem_ty;
    let slot = ctx.alloca_in_entry(slot_ty, Some("__oob_elem"));
    let oob_blk = ctx.f.add_block();
    let chk2_blk = ctx.f.add_block();
    let in_blk = ctx.f.add_block();
    let merge = ctx.f.add_block();
    let neg = ctx.cmp(IPred::Slt, idx_val.clone(), Operand::ConstI64(0));
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: neg,
            then_blk: oob_blk,
            else_blk: chk2_blk,
        },
    );
    ctx.cur_block = chk2_blk;
    let over = ctx.cmp(IPred::Sge, idx_val.clone(), Operand::Value(len));
    ctx.f.set_term(
        chk2_blk,
        Terminator::CondBr {
            cond: over,
            then_blk: oob_blk,
            else_blk: in_blk,
        },
    );
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
    ctx.f
        .append_void(oob_blk, InstKind::Store(oob_val, Operand::Value(slot), 0));
    ctx.f.set_term(oob_blk, Terminator::Br(merge));
    ctx.cur_block = in_blk;
    let (offset_base, offset) = ctx.emit_arr_slot_byte_offset(arr_val, idx_val, 3, is_non_deque);
    let cur = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur,
        InstKind::LoadDyn(elem_ty, offset_base, offset),
        elem_ty,
        None,
    );
    let stored = if promote_bool {
        ctx.cur_block = cur;
        ctx.box_to_any(Operand::Value(v))
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
