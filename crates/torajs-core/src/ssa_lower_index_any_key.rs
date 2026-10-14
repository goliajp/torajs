//! The `any`-keyed index lanes, split out of `ssa_lower_index.rs`
//! when the bool-element promotion pushed that file past the
//! 500-line limit. Reading through a tagged receiver or by a
//! runtime key is a separate question from the typed-array bounds
//! branch that stayed behind; these three share the probe contract
//! (the key is borrowed, an owned temp key is released after it).
//!
//! Re-exported from `ssa_lower_index` so every existing caller path
//! is unchanged.

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::ssa::{FuncId, InstKind, Module, Operand, Type, ValueId};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

/// `__torajs_any_member_get_pair(recv, key, value_out) -> tag`: both
/// channels of the any-lane read in one call (one dict probe on an
/// ordinary object), the value coming back through a stack slot.
const GET_PAIR: &str = "__torajs_any_member_get_pair";

pub(crate) fn declare_get_pair(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(
        module,
        fn_table,
        GET_PAIR,
        &[Type::Any, Type::Ptr, Type::Ptr],
        Type::I64,
    );
}

/// The dynamic-key read: [`GET_PAIR`] in place of the tag / value
/// call pair, consumed by the same accessor-or-data result emit.
fn emit_any_member_get_pair(ctx: &mut LowerCtx, obj_val: &Operand, key: ValueId) -> Operand {
    let slot = ctx.alloca_in_entry(Type::I64, Some("__get_value"));
    let tag = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.fn_table[GET_PAIR],
            vec![*obj_val, Operand::Value(key), Operand::Value(slot)],
        ),
        Type::I64,
        None,
    );
    let value = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    );
    ctx.emit_throw_check(None);
    crate::ssa_lower_accessor::emit_any_get_result(ctx, obj_val, key, tag, value)
}

/// Typed array receiver with a KEY that no static lane can answer —
/// an `any` key (`a[i]`, whose runtime tag decides between an element
/// read, a property read, and a miss) or a string key (§7.1.19 makes
/// `a["length"]` ≡ `a.length` and `a["0"]` an element read, and only
/// the key's spelling tells them apart). Box both sides at the lane
/// boundary and hand them to the keyed kernel's ToPropertyKey
/// dispatch, mirroring the struct-receiver arm.
///
/// The kernel reads raw typed slots through the header's element-kind
/// field, which every array reaching here carries: a heap array
/// records it inside `box_to_any`, and a non-escaping array literal
/// is born with it baked in (`ssa_lower_array::alloc_stack_arr` —
/// such a block is FLAG_STATIC_LITERAL, which the runtime marker
/// refuses to write).
pub(crate) fn lower_typed_arr_any_key(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    arr_val: Operand,
    index: ExprId,
) -> Operand {
    let boxed = ctx.box_to_any(arr_val);
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    // A string key arrives as a Str slot; the kernel takes an
    // AnyValue. The box helpers are pure encodings, so the raw
    // operand keeps its own stake and its own drop below.
    //
    // Which box, though, is the shared predicate's call and not this
    // lane's. `undefined` and `null` both lower to a null pointer and
    // only the source expression's TYPE tells them apart (ANY_UNDEF
    // vs ANY_NULL); the plain helper cannot see it, so this lane
    // spelled every `x[undefined]` as `x["null"]` while the write
    // side — which has ridden `lower_any_key` since rotation 346 —
    // stored it under "undefined". The two halves of one key
    // disagreed, and `x[undefined] = 9; x[undefined]` answered
    // undefined on a program that answers 9.
    let k_boxed = if ctx
        .expr_types
        .get(&index)
        .is_some_and(crate::check_type_of_index::key_needs_boxing)
    {
        ctx.box_to_any_from_expr(index, k_raw)
    } else if k_ty == Type::Any {
        k_raw
    } else {
        ctx.box_to_any(k_raw)
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.any_index_get_keyed, vec![boxed, k_boxed]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    // The kernel answers cells with an explicit +1, so the consumer
    // releases; the probe borrows the key, so only an owned temp key
    // is released here (the numeric lane's convention).
    ctx.owned_member_reads.insert(eid);
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    Operand::Value(v)
}

/// String / Substr receiver. A NUMBER key takes the character lane:
/// routed through the index-get runtime pair (RFC 20260707 residual),
/// where — unlike the charAt / slice method family, which answers the
/// empty string on OOB per §22.1.3.2 — `s[i]` OOB answers JS
/// `undefined` per §10.4.3 [[Get]], the immortal Substr-shaped
/// sentinel whose view walks to "undefined" (ToString consumers stay
/// branch-free) while identity consumers (strict-eq / typeof /
/// nullish probes) compare its address.
///
/// Any other key can't pick that lane: its spelling — or, for an
/// `any` key, its runtime tag — decides between the character,
/// `length`, a reified method and a miss. Box both sides and let the
/// keyed kernel's §7.1.19 ToPropertyKey dispatch answer, exactly as
/// the Arr receiver does. Pre-fix those keys rejected the whole
/// program ("index must be number, got String").
///
/// Lives here rather than in `ssa_lower_index` because that file's
/// `lower_from_value` sits against the 200-line rule and the file
/// itself against the 500-line one.
pub(crate) fn lower_str_recv_index(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    arr_val: Operand,
    arr_ty: Type,
    index: ExprId,
) -> Operand {
    // A possibly-absent str receiver: §13.3.2.1 evaluates the key
    // (step 3) before the read's GetValue reaches §7.1.20 ToObject, so
    // the key lowers first and is parked for the lane below, then the
    // guard runs. The kernel is the one the `.length` read uses; it
    // recognises NULL and both "undefined" sentinel blocks. Without it
    // `s[0]` on an absent `string | undefined` answered undefined.
    if crate::ssa_lower_nullable_guard::is_nullable_str_source(ctx, obj) {
        let key = ctx.lower_expr(index);
        crate::ssa_lower_nullable_guard::emit_nullable_str_guard(ctx, obj, &arr_val);
        ctx.redispatch_lowered = Some((index, key));
    }
    if ctx
        .expr_types
        .get(&index)
        .is_some_and(crate::check_type_of_index::string_key_domain)
    {
        return lower_typed_arr_any_key(ctx, eid, arr_val, index);
    }
    let (idx_val, nonint) = crate::ssa_lower_index_nonintegral::lower_typed_read_index(ctx, index);
    // A Substr receiver may be a loop's frame view with no heap block
    // to box, so it keeps the view read: a key past it or not an
    // integer reads undefined without consulting the prototype.
    if ctx.ast.array_prop_reads_may_raise && arr_ty == Type::Str {
        return crate::ssa_lower_index_nonintegral::emit_str_keyed_read(
            ctx, arr_val, idx_val, nonint,
        );
    }
    let idx_val = crate::ssa_lower_index_nonintegral::str_view_index(ctx, idx_val, nonint);
    let fid = if arr_ty == Type::Str {
        ctx.intrinsics.str_index_view
    } else {
        ctx.intrinsics.substr_index_view
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(fid, vec![arr_val, idx_val]),
        Type::Substr,
        None,
    );
    Operand::Value(v)
}

/// A Number key on an `any` receiver, split by what the SSA says
/// about it: an I64 / I32 rides the integer index kernel; an F64 is
/// not provably integral and goes to the keyed kernel boxed as the
/// double it is — that kernel routes an integral double to the index
/// lane itself and ToStrings the rest (§7.1.19). Before this every
/// Number key took `coerce_to_i64` and `o[1.5]` read and wrote
/// `o[1]`, a different property ("1.5" is not an array index). The
/// double box is a pure encode with no stake to release.
pub(crate) enum AnyNumericKey {
    Index(Operand),
    Boxed(Operand),
}

impl AnyNumericKey {
    /// Lower `index` once (a compound-assign pinned key is reused —
    /// §13.15.2 evaluates the Reference once) and pick the kernel.
    pub(crate) fn lower(ctx: &mut LowerCtx<'_>, index: ExprId) -> Self {
        let raw = match &ctx.pins.compound_key {
            Some((mid, mop)) if *mid == index => *mop,
            _ => ctx.lower_expr(index),
        };
        if matches!(ctx.operand_ty(&raw), Type::F64) {
            AnyNumericKey::Boxed(ctx.box_to_any(raw))
        } else {
            AnyNumericKey::Index(ctx.coerce_to_i64(raw))
        }
    }

    /// `recv[key]` — a fresh owned box (both kernels answer +1).
    pub(crate) fn emit_get(self, ctx: &mut LowerCtx<'_>, recv: Operand) -> ValueId {
        let (fid, key) = match self {
            AnyNumericKey::Index(i) => (ctx.intrinsics.any_index_get, i),
            AnyNumericKey::Boxed(k) => (ctx.intrinsics.any_index_get_keyed, k),
        };
        let cur_block = ctx.cur_block;
        ctx.f.append_inst(
            cur_block,
            InstKind::Call(fid, vec![recv, key]),
            Type::Any,
            None,
        )
    }

    /// `recv[key] = (tag, value)` — the pair transfers into the store.
    pub(crate) fn emit_set(
        self,
        ctx: &mut LowerCtx<'_>,
        recv: Operand,
        tag: Operand,
        value: Operand,
        recv_slot: Operand,
    ) {
        let (fid, soft, key) = match self {
            AnyNumericKey::Index(i) => (
                ctx.intrinsics.any_index_set,
                ctx.intrinsics.any_index_set_soft,
                i,
            ),
            AnyNumericKey::Boxed(k) => (
                ctx.intrinsics.any_index_set_keyed,
                ctx.intrinsics.any_index_set_keyed_soft,
                k,
            ),
        };
        crate::ssa_lower_set_flavor::emit_any_set(
            ctx,
            fid,
            soft,
            vec![recv, key, tag, value, recv_slot],
        );
    }
}

/// Lower a property-key expression for the keyed kernels' §7.1.19
/// ToPropertyKey dispatch. An `any` key is already a box; an
/// UNDEFINED-typed key (the t262 dstr `{}[thrower()]` shape — a
/// throwing callee types its result Undefined) lowers as a Ptr and
/// boxes to ANY_UNDEF here, which the kernel coerces to the fixed
/// string key "undefined". A STRUCT-typed key (cluster #4 — an
/// object literal carrying its own `toString`) is a heap cell; the
/// box is a pure tag encode and the kernel's runtime ToPropertyKey
/// runs OrdinaryToPrimitive over it.
pub(crate) fn lower_any_key(ctx: &mut LowerCtx<'_>, index: ExprId) -> Operand {
    let raw = ctx.lower_expr(index);
    if ctx
        .expr_types
        .get(&index)
        .is_some_and(crate::check_type_of_index::key_needs_boxing)
    {
        return ctx.box_to_any_from_expr(index, raw);
    }
    raw
}

pub(crate) fn lower_array_any_index(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
) -> Operand {
    Operand::Value(crate::ssa_lower_anyv_inline::arr_get_any_elem(
        ctx, arr_val, idx_val,
    ))
}

/// The tag / value pair read of `arr[idx]` on an `Array<Any>`, boxed
/// (borrowed): the slow arm of
/// [`crate::ssa_lower_anyv_inline::arr_get_any_elem`].
pub(crate) fn emit_any_elem_pair(
    ctx: &mut LowerCtx<'_>,
    arr_val: Operand,
    idx_val: Operand,
) -> ValueId {
    let cur_block = ctx.cur_block;
    let tag = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_get_any_tag, vec![arr_val, idx_val]),
        Type::I64,
        None,
    );
    let cur_block = ctx.cur_block;
    let value = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.arr_get_any_value, vec![arr_val, idx_val]),
        Type::I64,
        None,
    );
    // the tag read runs an accessor element's getter, which may throw
    ctx.emit_throw_check(None);
    let cur_block = ctx.cur_block;

    ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::Value(tag), Operand::Value(value)],
        ),
        Type::Any,
        None,
    )
}

/// Dynamic string key on an `any` receiver — probe by the runtime
/// Str cell (borrow); a Substr view materializes to an owned temp
/// released after the probe.
pub(crate) fn lower_any_index_str_key(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    index: ExprId,
) -> Operand {
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    let owned = k_ty == Type::Substr;
    let key_op = ctx.coerce_to_str(k_raw, k_ty);
    let Operand::Value(key_v) = key_op else {
        panic!("ssa-lower: string index key lowered to a non-value operand");
    };
    let out = emit_any_member_get_pair(ctx, &obj_val, key_v);
    if owned {
        ctx.emit_drop_value(key_op, Type::Str);
    }
    // The probe borrows the key (dynobj/arrprops hash lookup — no rc
    // traffic, no storage), so an Ident key keeps its stake and its
    // scope drop; an owned temp key (concat result / fresh view) is
    // released here (RFC 20260705 ledger #3, 32B/iter probe).
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    out
}

/// Symbol key on an `any` receiver — §7.1.19 step 2 hands the key to
/// the lookup uncoerced, so the probe reads the Symbol cell itself and
/// the runtime side keys off its `type_tag`. Twin of
/// [`lower_any_index_str_key`] minus the coerce: there is no view /
/// wrapper form of a Symbol to materialize.
pub(crate) fn lower_any_index_symbol_key(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    index: ExprId,
) -> Operand {
    let k_raw = ctx.lower_expr(index);
    let k_ty = ctx.operand_ty(&k_raw);
    let Operand::Value(key_v) = k_raw else {
        panic!("ssa-lower: symbol index key lowered to a non-value operand");
    };
    let out = emit_any_member_get_pair(ctx, &obj_val, key_v);
    // Borrow contract as the string-key twin: an Ident key keeps its
    // stake and its scope drop, an owned temp (a fresh `Symbol(desc)`
    // in the index position) is released after the probe reads it.
    if k_ty.is_refcounted() && ctx.expr_transfers_ownership(index) {
        ctx.emit_drop_value(k_raw, k_ty);
    }
    out
}
/// The struct-like receiver's dynamic-index read (chunk 753; carved
/// out of [`lower_from_value`] when the rotation-346 undefined-key
/// arm pushed it past the 200-line fn limit — body verbatim). The
/// struct cell encodes as a NaN-box (pure pointer encoding, zero rc
/// traffic — the receiver binding keeps its stake and the box is a
/// borrow) and every key shape rides the any-index runtime lane.
pub(crate) fn lower_struct_like_index(
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
    // §6.1.7 / §7.1.19 step 2 — a symbol key reaches the probe as
    // its own cell, uncoerced. The any-receiver lane in
    // `ssa_lower_index::lower_from_value` has had this arm since it
    // was written and this one did not, so `({ a: 1 })[sym]`
    // type-checked — the gate admits Struct x Symbol — and then
    // died coercing a Symbol to i64. The seventh mirror of one
    // domain in as many rotations.
    //
    // These two sets of arms have to stay the same SET. What
    // legitimately differs is only their rc contract: over there the
    // receiver may be an owned temp to release, here the box is a
    // borrow off a live binding (see the caller's note), which is
    // why they are not one function.
    if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::Symbol)) {
        let v = lower_any_index_symbol_key(ctx, boxed, index);
        ctx.owned_member_reads.insert(eid);
        return v;
    }
    // Cluster #1 blade 5 — an `any`-typed key on the boxed struct
    // rides the keyed kernel's ToPropertyKey dispatch (mirror of
    // the Any-receiver arm below). An UNDEFINED key joins
    // (rotation 346): `lower_any_key` boxes it to ANY_UNDEF. A
    // STRUCT key joins (cluster #4): `lower_any_key` tag-encodes
    // the heap cell and the kernel runs OrdinaryToPrimitive.
    if ctx
        .expr_types
        .get(&index)
        .is_some_and(crate::check_type_of_index::key_rides_keyed_kernel)
    {
        // Cluster #4 T4 — a compound write lane pinned the coerced
        // key for this eid: reuse it (one ToPropertyKey total), no
        // re-lower, no drop (the write lane owns the cell).
        let (k_raw, memoed) = match &ctx.pins.compound_key {
            Some((mid, mop)) if *mid == index => (*mop, true),
            _ => (lower_any_key(ctx, index), false),
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_index_get_keyed, vec![boxed, k_raw]),
            Type::Any,
            None,
        );
        ctx.emit_throw_check(None);
        ctx.owned_member_reads.insert(eid);
        if !memoed && ctx.expr_transfers_ownership(index) {
            ctx.emit_drop_value(k_raw, Type::Any);
        }
        return Operand::Value(v);
    }
    let v = AnyNumericKey::lower(ctx, index).emit_get(ctx, boxed);
    ctx.emit_throw_check(None);
    // `__torajs_arr_index_get` answers cells with an explicit +1
    // ("the returned copy takes another") — record the read as
    // owned so the consumer releases it (chunk 717 convention);
    // pre-record the +1 was stranded (~128B/iter on the
    // any-index churn probe). The boxed receiver itself is a
    // borrow (chunk 753 pure-encoding note above), no release.
    ctx.owned_member_reads.insert(eid);
    Operand::Value(v)
}
