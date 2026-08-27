//! `Expr::Ternary { cond, then_branch, else_branch }` lowering
//! pulled out of [`crate::ssa_lower::lower_expr_inner`]'s match arm
//! as chunk-69 of the decomp (chunks 1-68 = ... + `Expr::Closure`
//! M2 env construction).
//!
//! Lowers as `let __tmp; if (cond) __tmp = T else __tmp = E; __tmp`.
//! Result type comes from the branches (verified equal by
//! `check.rs`), with three mixed-arm widen wedges applied **after**
//! both branches lower (so the post-branch `cur_block` reflects any
//! nested ternaries / calls that moved it forward):
//!
//! - **W3 S8** — mixed `i64`/`f64` number pair joins at `f64`.
//!   `check.rs` verified both arms are `number`, but the float face
//!   (`n < 0 ? -n : n`) can leave one arm `f64` while the other
//!   stays `i64`. Whichever is `i64` gets `coerce_to_f64`'d in its
//!   branch's end block before the shared slot store.
//! - **S129-1** — mixed-Any wedge: `check.rs`'s `unify_ternary`
//!   widens to `Any` when either arm is `Any`. The slot must be
//!   `Any`-typed and the non-Any branch NaN-boxed via `box_to_any`
//!   so the post-block Load decodes a proper AnyValue. Same shape
//!   as the S128-5 logical mixed-Any widen and the S128-4 reduce
//!   init box-coerce.
//! - **Other** — fall-through: result type = then arm's type
//!   (caller already verified equality).
//!
//! Both branches store to a single `alloca_in_entry`-allocated
//! result slot; the post-block loads from it. (Same dominance
//! pattern as pending_break — alloca in entry so both branches'
//! current blocks dominate the load.)
//!
//! Returns `Operand` directly (terminal arm — caller's
//! `Expr::Ternary` match arm bottoms out here).

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    cond: ExprId,
    then_branch: ExprId,
    else_branch: ExprId,
) -> Operand {
    let raw = ctx.lower_expr(cond);
    let cond_op = ctx.coerce_to_bool(raw.clone());
    // Chunk 636 — release an owned condition temp after the
    // truthiness test (see ssa_lower_stmt_if.rs); emitted in the
    // pre-branch block, before the CondBr splits control flow.
    ctx.release_owned_temp(cond, &raw);
    let then_blk = ctx.f.add_block();
    let else_blk = ctx.f.add_block();
    let after_blk = ctx.f.add_block();
    let saved = ctx.cur_block;
    ctx.cur_block = then_blk;
    let then_val = ctx.lower_expr(then_branch);
    let mut then_end = ctx.cur_block;
    ctx.cur_block = else_blk;
    let else_val = ctx.lower_expr(else_branch);
    let mut else_end = ctx.cur_block;
    let (then_val, else_val, res_ty, then_boxed, else_boxed) = widen_branches(
        ctx,
        eid,
        then_val,
        else_val,
        &mut then_end,
        &mut else_end,
        then_branch,
        else_branch,
    );
    // Chunk 722 — owned unification: when either branch answers an
    // owned value (Call / New / fresh Any box from the widen), the
    // join result must be owned on BOTH paths so the consumer's
    // single release balances (probe p722a: discarded
    // `c ? mk(i) : mk2(i)` had no release site, 15.3MB churn vs
    // 6.2MB flat). The borrow branch takes a tail-block inc; the
    // eid joins the owned track `expr_owned_shape` consults. A
    // join over two borrows stays a borrow — zero rc traffic.
    if res_ty.is_refcounted() {
        let then_owned = then_boxed || ctx.expr_owned_shape(then_branch);
        let else_owned = else_boxed || ctx.expr_owned_shape(else_branch);
        if then_owned || else_owned {
            if !then_owned {
                ctx.emit_owned_result_inc_in(then_end, then_val.clone(), res_ty);
            }
            if !else_owned {
                ctx.emit_owned_result_inc_in(else_end, else_val.clone(), res_ty);
            }
            ctx.owned_member_reads.insert(eid);
        }
    }
    let res_slot = ctx.alloca_in_entry(res_ty, Some("__tern"));
    ctx.f.append_void(
        then_end,
        InstKind::Store(then_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(then_end, Terminator::Br(after_blk));
    ctx.f.append_void(
        else_end,
        InstKind::Store(else_val, Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(else_end, Terminator::Br(after_blk));
    ctx.f.set_term(
        saved,
        Terminator::CondBr {
            cond: cond_op,
            then_blk,
            else_blk,
        },
    );
    ctx.cur_block = after_blk;
    let r = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(res_ty, Operand::Value(res_slot), 0),
        res_ty,
        None,
    );
    Operand::Value(r)
}

/// The two trailing bools flag a branch whose value was NaN-boxed
/// here (mixed-Any wedge) — the box is fresh, so that branch is
/// owned by construction regardless of the source expression's
/// shape. Chunk 722: `box_to_any` transfers one reference into the
/// box (chunk-563 contract), so a borrow-shape branch takes +1
/// before boxing — without it the box stole the source binding's
/// stake and the owned-join release turned that into a double-dec.
///
/// `then_end` / `else_end` are IN-OUT: a widen that boxes through an
/// expr-aware lane can OPEN BLOCKS (`box_f64_or_undef` splits into
/// undef/num/merge for a possibly-sentinel F64 — the typed-arr OOB
/// read), leaving the boxed value defined in a NEW tail block. The
/// caller's Store/Br must land on that tail — writing them on the
/// stale end used to overwrite the box's own CondBr and orphan the
/// merge block (regalloc "ValueId not allocated"; SIGSEGV with the
/// egraph off — the L3b ① spread-fixture discovery, latent for every
/// `cond ? typedArr[i] : undefined` join since the OOB-read RFC).
fn widen_branches(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    then_val: Operand,
    else_val: Operand,
    then_end: &mut crate::ssa::BlockId,
    else_end: &mut crate::ssa::BlockId,
    then_branch: ExprId,
    else_branch: ExprId,
) -> (Operand, Operand, Type, bool, bool) {
    let tt = ctx.operand_ty(&then_val);
    let et = ctx.operand_ty(&else_val);
    if tt == Type::F64 && et == Type::I64 {
        ctx.cur_block = *else_end;
        let e = ctx.coerce_to_f64(else_val);
        *else_end = ctx.cur_block;
        return (then_val, e, Type::F64, false, false);
    }
    if tt == Type::I64 && et == Type::F64 {
        ctx.cur_block = *then_end;
        let t = ctx.coerce_to_f64(then_val);
        *then_end = ctx.cur_block;
        return (t, else_val, Type::F64, false, false);
    }
    // S2.27 (RFC 20260727-dstr-assignment 刀 5) — exactly one branch
    // types Undefined (the checker's unify widened the join to Any):
    // box BOTH sides expr-aware, so the undefined side carries its
    // ANY_UNDEF tag through the eid gate instead of falling to the
    // slot-type mismatch below (`cond ? undefined : 42` loaded the
    // ConstPtrNull through an I64 slot and answered 0). Both boxes
    // are fresh, so both branches join owned.
    let then_undef = matches!(
        ctx.expr_types.get(&then_branch),
        Some(crate::check::Type::Undefined)
    );
    let else_undef = matches!(
        ctx.expr_types.get(&else_branch),
        Some(crate::check::Type::Undefined)
    );
    // rotation 362 — exactly one branch types Null against a
    // register-repr or struct branch (the checker joined to Any, the
    // S2.27 posture): box BOTH sides expr-aware. The Null side is a
    // compile-time ConstPtrNull, which `box_to_any` folds to ANY_NULL;
    // without the box the I64/Bool lanes stored its zero bits as a
    // number (probe: `c > 5 ? 7 : null` printed 0) and the F64 lane
    // refused loudly. Ptr-repr joins (Nullable<Str> etc.) stay on
    // their native slot — the gate requires the OTHER side to be a
    // scalar or struct.
    let then_null = matches!(
        ctx.expr_types.get(&then_branch),
        Some(crate::check::Type::Null)
    );
    let else_null = matches!(
        ctx.expr_types.get(&else_branch),
        Some(crate::check::Type::Null)
    );
    let scalar_or_struct = |t: &Type| {
        matches!(
            t,
            Type::I64 | Type::I32 | Type::F64 | Type::Bool | Type::Obj(_)
        )
    };
    let null_join = (then_null && !else_null && scalar_or_struct(&et))
        || (else_null && !then_null && scalar_or_struct(&tt));
    // rotation 233 (S2.27 field depth) — two struct-shaped branches
    // whose layouts differ (the checker joined them to Any: same
    // field names, field-wise joinable): box BOTH sides expr-aware,
    // so each branch's fields read back through the any-member
    // runtime-layout path instead of one branch's raw layout being
    // read by the other's (probe: `(false ? {v: 1} : {v: undefined})
    // .v` answered the sentinel-cell pointer as a number).
    let struct_join = matches!(tt, Type::Obj(_)) && matches!(et, Type::Obj(_)) && tt != et;
    // 506-02 — two class instances the checker joined to a common
    // ancestor (`ClassRef(lca)`, `check_type_of_ternary`): NO box.
    // Both pointers carry the ancestor's layout prefix — the same
    // invariant `let b: Base = new Leaf()` stores through — so the
    // slot takes the ancestor's Obj repr and every consumer
    // dispatches on the static ancestor type (vtable). The struct
    // join below is for the rotation-233 shape (two literal layouts
    // the checker widened to Any), which this gate leaves alone.
    if struct_join
        && let Some(crate::check::Type::ClassRef(lca)) = ctx.expr_types.get(&eid)
        && ctx.ast.class_parents.contains_key(lca)
    {
        let res = crate::ssa_lower_parse_type::parse_type(
            Some(lca.as_str()),
            ctx.aliases,
            ctx.arr_layouts,
            ctx.fn_sigs,
            ctx.generic_struct_decls,
            ctx.struct_layouts,
            ctx.inst_memo,
        );
        if matches!(res, Type::Obj(_)) {
            return (then_val, else_val, res, false, false);
        }
    }
    // rotation 284 — two array-shaped branches whose element reprs
    // differ (the checker joined `Array(T)` × `Array(Any)` to Any):
    // box BOTH sides expr-aware, so each block reads back through
    // the kind-aware any lanes instead of one flavor's raw slot
    // layout being read by the other's (probe: `(!t ? [1, 2] :
    // anyArr)[0]` answered the slot's NaN-box bits as a number).
    let arr_join = matches!(tt, Type::Arr(_)) && matches!(et, Type::Arr(_)) && tt != et;
    // rotation 284 — one branch an array, the other a class instance
    // (the dstr default-guard `Array(Any)` × generator ClassRef join
    // the checker widened to Any): same both-sides box, so the
    // consumer's any lanes see tagged values whichever branch runs.
    let heap_mixed_join = (matches!(tt, Type::Arr(_)) && matches!(et, Type::Obj(_)))
        || (matches!(tt, Type::Obj(_)) && matches!(et, Type::Arr(_)));
    // rotation 400 (398-07) — two DIFFERENT concrete scalars the
    // checker joined to Any (`x === undefined ? "undef" : 1`): box
    // both sides expr-aware, mirroring the unify_ternary arm exactly
    // — the gate reads the CHECKER types, not the SSA reprs, so a
    // same-checker-type width split (I64 × I32 Number) never
    // matches. Without the box the Str branch's pointer bits read
    // back through the other branch's slot type (probe: printed the
    // raw pointer as a number).
    let scalar_ck = |e: &ExprId| {
        matches!(
            ctx.expr_types.get(e),
            Some(
                crate::check::Type::String
                    | crate::check::Type::Number
                    | crate::check::Type::Boolean
            )
        )
    };
    let scalar_mixed_join = scalar_ck(&then_branch)
        && scalar_ck(&else_branch)
        && ctx.expr_types.get(&then_branch) != ctx.expr_types.get(&else_branch);
    if (struct_join
        || arr_join
        || heap_mixed_join
        || scalar_mixed_join
        || then_undef != else_undef
        || null_join)
        && tt != Type::Any
        && et != Type::Any
    {
        ctx.cur_block = *then_end;
        if tt.is_refcounted() && !ctx.expr_transfers_ownership(then_branch) {
            ctx.emit_rc_inc(then_val.clone());
        }
        let t = ctx.box_to_any_from_expr(then_branch, then_val);
        *then_end = ctx.cur_block;
        ctx.cur_block = *else_end;
        if et.is_refcounted() && !ctx.expr_transfers_ownership(else_branch) {
            ctx.emit_rc_inc(else_val.clone());
        }
        let e = ctx.box_to_any_from_expr(else_branch, else_val);
        *else_end = ctx.cur_block;
        return (t, e, Type::Any, true, true);
    }
    // The box is expr-AWARE here for the same reason S2.27 above is:
    // an `undefined` branch is a compile-time ConstPtrNull, and the
    // plain `box_to_any` tags that as ANY_NULL. The wedge above only
    // covers the case where NEITHER side is already Any, so
    // `cond ? undefined : anyValue` fell through to here and answered
    // `null` — `typeof (b ? undefined : v)` printed "object", and a
    // `JSON.stringify` replacer written the MDN way
    // (`(k, v) => k === drop ? undefined : v`) emitted `"k":null`
    // instead of dropping the key.
    if tt == Type::Any || et == Type::Any {
        if tt != Type::Any {
            ctx.cur_block = *then_end;
            if tt.is_refcounted() && !ctx.expr_transfers_ownership(then_branch) {
                ctx.emit_rc_inc(then_val.clone());
            }
            let t = ctx.box_to_any_from_expr(then_branch, then_val);
            *then_end = ctx.cur_block;
            return (t, else_val, Type::Any, true, false);
        }
        if et != Type::Any {
            ctx.cur_block = *else_end;
            if et.is_refcounted() && !ctx.expr_transfers_ownership(else_branch) {
                ctx.emit_rc_inc(else_val.clone());
            }
            let e = ctx.box_to_any_from_expr(else_branch, else_val);
            *else_end = ctx.cur_block;
            return (then_val, e, Type::Any, false, true);
        }
        return (then_val, else_val, Type::Any, false, false);
    }
    (then_val, else_val, tt, false, false)
}
