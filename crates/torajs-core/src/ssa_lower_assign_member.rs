//! `Expr::Assign { target: Expr::Member { obj, name: field }, value }`
//! lowering pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Assign` match arm as chunk-80 of the decomp.
//!
//! Dispatch ladder (each path returns the assigned value as the
//! expression result, modulo the dynobj / Arr-prop paths that return
//! `ConstI64(0)` to match the legacy in-line emit shape):
//!
//! 1. **Type::Any** (`P3.2`, tag-gated per RFC 20260704 C4+) — pack
//!    RHS as `(tag, value)` and call `__torajs_any_member_set`, which
//!    dispatches on the receiver's heap tag. Nested Type::Any payload
//!    routes through `any_payload_rc_inc`. Frozen / non-writable
//!    write throws via `emit_throw_check`. Post-resize relocation
//!    write-back rides the AnyValue slot, re-stored to a plain-Ident
//!    receiver's local.
//! 2. **Type::FnSig** (`T-27.b`) — top-level FnDecl routes through
//!    the `fnprops` side table keyed by fn pointer.
//! 3. **Type::Arr + field=="length"** (`S133-3`) — spec §9.4.2.4 length
//!    setter: route to `arr_set_length_validate` (refcount elem types)
//!    or `arr_set_length_truncate_scalar` (i64/f64/bool elements).
//! 4. **Type::RegExp + field=="lastIndex"** (`P9.4`) — coerce RHS to
//!    f64 (uncoerced-store spec shape; ToLength happens where the
//!    regex kernels consume it), call `regex_set_last_index`, return
//!    the value as the expression result (mirrors a field store).
//! 5. **Type::Obj** (struct receiver) — the accessor-setter direct
//!    call (`P8.2`, args through the `arg_conv` contract) and the
//!    direct struct field store both live in
//!    [`crate::ssa_lower_assign_member_field`].
//! 6. **Everything else** — box the receiver and take lane 1.
//!    Knowing the shape at compile time is not a licence to skip
//!    §10.1.9.2's chain consult, and the shapes without an arm used
//!    to be a compile error rather than a store.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    field: String,
    value: ExprId,
) -> Operand {
    // Step 7d-A — capture the LHS variable name (if `obj` is a plain
    // Ident) so the Type::Any dynobj-set / dynobj_define paths below
    // can write the post-resize ptr back to the variable's storage
    // as a fresh NaN-box `AnyValue`.
    let obj_ident: Option<String> = if let Expr::Ident(n) = ctx.ast.get_expr(obj) {
        Some(n.clone())
    } else {
        None
    };
    let obj_val = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_val);

    if matches!(obj_ty, Type::Any) {
        // A fresh owned receiver box (inline as-cast, member-read
        // chain) releases inside the set tail; an Ident borrow keeps
        // its binding's stake (and rides the write-back instead).
        let recv_owned = ctx.expr_transfers_ownership(obj);
        return crate::ssa_lower_assign_member_any::lower_dynobj_assign(
            ctx, eid, obj_val, &field, value, &obj_ident, recv_owned,
        );
    }
    if matches!(obj_ty, Type::FnSig(_)) {
        return lower_fnsig_props_assign(ctx, eid, obj_val, &field, value);
    }
    if matches!(obj_ty, Type::Arr(_)) && field == "length" {
        return lower_arr_length_assign(ctx, eid, obj_val, obj_ty, value);
    }
    if obj_ty == Type::RegExp && field == "lastIndex" {
        return lower_regex_last_index_assign(ctx, obj_val, value);
    }
    if let Type::Obj(sid) = obj_ty {
        return lower_obj_assign(ctx, eid, obj, obj_val, sid, &field, value);
    }
    // Every other receiver: box the cell and take the any lane.
    //
    // §10.1.9.2 OrdinarySet does not care that the compiler knew the
    // receiver's shape — the chain consult, the frozen check and the
    // §10.1.8.1 own-first order are the same whether the program
    // spelled `m` or an `any` binding holding it. Promise, Closure
    // and Arr each arrived at that one at a time (a direct props
    // write skipped the chain, so `f.caller = {}` minted an own key
    // where %Function.prototype% has a %ThrowTypeError% setter,
    // while the SAME program through an `any` binding threw); this
    // is the rest of the language arriving at it together. Before,
    // a shape the ladder had no arm for was a COMPILE error —
    // `(new Map() as any).zz = 1` did not build, and neither did the
    // Set / Date / RegExp / Str / Symbol / BigInt spelling, while
    // every one of them worked through an `any` binding. It is the
    // write-side twin of rotation 527's read-side `d7917706d`.
    //
    // No write-back and no release ride the tail: these cells never
    // relocate (their bag lives in its own slot, unlike a dynobj
    // whose store can move), and `box_to_any` on a typed operand is
    // a pure bit-encode borrow.
    let boxed = ctx.box_to_any(obj_val);
    crate::ssa_lower_assign_member_any::lower_dynobj_assign(
        ctx, eid, boxed, &field, value, &None, false,
    )
}

/// The assignment expression's value is its rhs (§13.15.2 step 8 —
/// the rvalue, after GetValue). Five of this ladder's lanes answered
/// `0` instead: `b = (o.k = [1,2,3])` left `b` holding the integer
/// zero, silently. The struct-field lane and the regex-lastIndex lane
/// already answered their value; these join them on the Ident lane's
/// contract (`mint_consumer_stake`): the consumer receives an OWNED
/// reference, so keepers transfer without inc'ing and discard sites
/// release — not a borrow whitelist. The mint runs before the owned
/// temp's own release so the value never passes through zero.
fn finish_assign_value(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    v: Operand,
    v_ty: Type,
    transfers: bool,
) -> Operand {
    if v_ty.is_refcounted() {
        if !transfers {
            // A borrow-shape rhs: the consumer's reference is a fresh
            // stake, type-aware (an Any operand is NaN-box bits — a
            // raw header inc would treat them as an address).
            ctx.emit_owned_result_inc(v.clone(), v_ty);
        }
        // An owned temp TRANSFERS: the stake the lanes used to
        // release after the store is the very reference the consumer
        // now owns, so there is neither an inc nor a release here.
        // Minting a fresh +1 and keeping the release looks equivalent
        // on paper and is not: the eid below also routes discard
        // sites' cleanup to this value, and the pair double-released
        // it (a statement-position `o.k = [7,8]` freed the bucket's
        // array out from under it).
        ctx.owned_member_reads.insert(eid);
    }
    v
}

fn lower_fnsig_props_assign(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj_val: Operand,
    field: &str,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — SHARE, mirror of the closure-props arm above.
    let transfers = ctx.expr_transfers_ownership(value);
    let v_ty = ctx.operand_ty(&v_raw);
    let (tag, val_op) = ctx.box_to_tag_value(v_raw.clone());
    let key_str = ctx.intern_string_literal(field);
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.fnprops_set,
            vec![obj_val, Operand::Value(key_str), tag, val_op],
        ),
    );
    finish_assign_value(ctx, eid, v_raw, v_ty, transfers)
}

fn lower_arr_length_assign(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj_val: Operand,
    obj_ty: Type,
    value: ExprId,
) -> Operand {
    // Chunk 566 — no consume: the rhs is a number (checker-gated),
    // so the historical consume was a Copy no-op; an Any-typed Ident
    // rhs must keep its stake (the validate helper only reads).
    let (tag, val_op, v_raw, v_ty) = ctx.lower_to_tag_value_raw(value);
    let _ = &tag;
    // ES §10.4.2.5 — scalar element types keep the truncate helper;
    // every other element type (Any / Str / Arr / ...) routes to the
    // full resize helper (per-slot release on truncate, undefined
    // fill on Array<Any> grow — RFC 20260712 backlog item landed).
    let elem_ty = if let Type::Arr(elem_arr_id) = obj_ty {
        Some(ctx.arr_layouts[elem_arr_id.0 as usize])
    } else {
        None
    };
    let truncate_scalar = matches!(
        elem_ty,
        Some(Type::I64) | Some(Type::F64) | Some(Type::Bool)
    );
    let helper = if truncate_scalar {
        ctx.intrinsics.arr_set_length_truncate_scalar
    } else {
        ctx.intrinsics.arr_set_length_any
    };
    let argv = vec![obj_val, tag, val_op];
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(helper, argv));
    ctx.emit_throw_check(None);
    finish_assign_value(ctx, eid, v_raw, v_ty, false)
}

fn lower_regex_last_index_assign(
    ctx: &mut LowerCtx<'_>,
    obj_val: Operand,
    value: ExprId,
) -> Operand {
    let v_raw = ctx.lower_expr(value);
    // Chunk 566 — no consume: any_to_number only READS an Any rhs,
    // so a borrow-shape source keeps its stake; an owned Any temp's
    // box releases after the store below.
    let transfers = ctx.expr_transfers_ownership(value);
    // lastIndex is an ordinary data property — the store is
    // uncoerced (`r.lastIndex = 2.9` reads back 2.9); ToLength
    // happens at the regex kernels' consumption sites. An Any RHS
    // goes through ToNumber for the f64 slot.
    let v_ty = ctx.operand_ty(&v_raw);
    let v_keep = v_raw.clone();
    let v_f64 = if v_ty == Type::Any {
        let cur_block = ctx.cur_block;
        let f = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.any_to_number, vec![v_raw]),
            Type::F64,
            None,
        );
        Operand::Value(f)
    } else {
        ctx.coerce_to_f64(v_raw)
    };
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.regex_set_last_index,
            vec![obj_val, v_f64.clone()],
        ),
    );
    if transfers && v_ty.is_refcounted() {
        ctx.emit_drop_value(v_keep, v_ty);
    }
    v_f64
}

fn lower_obj_assign(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    sid: StructId,
    field: &str,
    value: ExprId,
) -> Operand {
    if let Some(v) = crate::ssa_lower_assign_member_field::try_lower_setter_call(
        ctx,
        eid,
        obj,
        obj_val.clone(),
        sid,
        field,
        value,
    ) {
        return v;
    }
    if let Some(v) =
        crate::ssa_lower_assign_member_objlit::try_lower(ctx, obj_val.clone(), sid, field, value)
    {
        return v;
    }
    // Rotation 373 — the checker admitted a name the layout never
    // carries (write mirror of the read side's blade-4 miss arm).
    // A `__priv_` mangled name resolving to a class METHOD is a
    // §13.15.2 PutValue → PrivateSet kind=method: the rhs evaluates
    // first (GetValue/compute already happened inside it for compound
    // forms), then the write itself throws a catchable TypeError. All
    // other misses — plain expando definitions and private getter-only
    // accessors — box the receiver and ride the any-member set lane,
    // whose runtime tail owns the +24 expando dict and the accessor
    // dispatch (getter-only throws the same readonly TypeError there).
    if !ctx.struct_layouts[sid.0 as usize]
        .iter()
        .any(|(n, _)| n == field)
    {
        if field.starts_with("__priv_")
            && let Some(cname) = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, obj)
            && ctx.fn_table.contains_key(&format!("__cm_{cname}__{field}"))
        {
            return lower_private_method_write_throw(ctx, value);
        }
        let boxed = ctx.box_to_any(obj_val);
        return crate::ssa_lower_assign_member_any::lower_dynobj_assign(
            ctx, eid, boxed, field, value, &None, false,
        );
    }
    crate::ssa_lower_assign_member_field::lower_struct_field_store(ctx, obj_val, sid, field, value)
}

/// §13.15.2 — the rhs evaluates first and its value drops (the throw
/// makes it unobservable), then the readonly-assign raiser records the
/// pending TypeError and the throw-check propagates it (the
/// `lower_self_name_write_throw` shape). `undefined`'s shape stands in
/// so the enclosing expression still types out.
fn lower_private_method_write_throw(ctx: &mut LowerCtx<'_>, value: ExprId) -> Operand {
    let v = ctx.lower_expr(value);
    let v_ty = ctx.operand_ty(&v);
    if !v_ty.is_copy() {
        ctx.emit_drop_value(v, v_ty);
    }
    let raiser = ctx.intrinsics.throw_readonly_assign;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(cur_block, InstKind::Call(raiser, vec![]));
    ctx.emit_throw_check(None);
    Operand::ConstPtrNull
}
