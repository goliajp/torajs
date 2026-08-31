//! `Expr::OptIndex { obj, index }` lowering — `obj?.[index]` optional
//! element access (ES2020 §13.3.9, chunk 703). The OptChain sibling
//! (`ssa_lower_optchain_arm.rs`) owns the `?.field` shape; this file
//! owns the element-access shape. `a?.["k"]` with an identifier-name
//! string literal folds to OptChain at parse time, so every index
//! reaching here is a genuinely dynamic key.
//!
//! Same contract as OptChain: the receiver evaluates exactly once; a
//! nullish receiver short-circuits to an ANY_UNDEF box WITHOUT
//! evaluating `index` (the index lowers inside the hit block); the
//! result is always a `Type::Any` box — downstream Any-aware ops
//! (typeof / strict-eq / nullish / print / arithmetic coercion)
//! handle it uniformly.
//!
//! Two receiver shapes:
//!
//! - **`Type::Any`** — NaN-box tag check (ANY_NULL=0 / ANY_UNDEF=5
//!   short-circuit); the hit path mirrors `ssa_lower_index`'s Any
//!   dispatch (string-literal member read / dynamic-string-key probe
//!   / `any_index_get`).
//! - **ptr-typed** (Arr / Str / Substr, incl. nullable exec/match
//!   results) — null-check via `ICmp::Eq(recv, ConstPtrNull)`; the
//!   hit path re-enters [`crate::ssa_lower_index::lower_from_value`]
//!   and boxes the element to Any. Statically non-null receivers
//!   fold the null branch away at LLVM level.
//!
//! Non-indexable receivers keep `lower_from_value`'s loud panic.

use crate::ast::ExprId;
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn lower(ctx: &mut LowerCtx<'_>, eid: ExprId, obj: ExprId, index: ExprId) -> Operand {
    let is_non_deque = ctx.arr_expr_is_non_deque(obj);
    let obj_op = ctx.lower_expr(obj);
    let obj_ty = ctx.operand_ty(&obj_op);
    let res_slot = ctx.alloca_in_entry(Type::Any, Some("__optindex"));
    let nullish = emit_nullish_cond(ctx, &obj_op, &obj_ty);
    let null_blk = ctx.f.add_block();
    let hit_blk = ctx.f.add_block();
    let after = ctx.f.add_block();
    let cb = ctx.cur_block;
    ctx.f.set_term(
        cb,
        Terminator::CondBr {
            cond: nullish,
            then_blk: null_blk,
            else_blk: hit_blk,
        },
    );
    // Miss path → ANY_UNDEF box (spec: short-circuit value is
    // undefined, not null).
    ctx.cur_block = null_blk;
    let undef_box = ctx.f.append_inst(
        null_blk,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    ctx.f.append_void(
        null_blk,
        InstKind::Store(Operand::Value(undef_box), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(null_blk, Terminator::Br(after));
    // Hit path — the index expression lowers HERE (short-circuit
    // never evaluates it).
    ctx.cur_block = hit_blk;
    let elem = if matches!(obj_ty, Type::Any) {
        lower_any_hit(ctx, eid, obj_op, index)
    } else {
        let v =
            crate::ssa_lower_index::lower_from_value(ctx, eid, obj, index, obj_op, is_non_deque);
        match ctx.operand_ty(&v) {
            Type::Any => v,
            vt => {
                // The eid-aware encoder, not the plain one: the
                // element may be an OOB read's `undefined` sentinel
                // (F64 bits, or the generic cell for the heap
                // shapes), and only this entry asks. `zs?.[9]`
                // printed NaN while `zs[9]` one line above printed
                // `undefined` — the box was where the answer was
                // lost, not the predicate, which names OptIndex.
                let boxed = ctx.box_to_any_from_expr(eid, v.clone());
                // Chunk 726 — a Str/Substr receiver's index read is
                // a FRESH view (owned, rc=1): record the eid owned so
                // the consumer's release balances (probe q726b:
                // discarded `s?.[i]` churn 15.2MB vs 6.2MB flat).
                // Rotation 184 — box_to_any is a pure encode for
                // every heap shape (anyv_box_str_slot included:
                // box_void_ptr never incs), so the fresh view's only
                // stake transfers INTO the box. The old drop here
                // freed the view under the live box (use-after-free
                // + a second consumer release). The typed-array
                // element arm measures flat as a borrow (q726e) and
                // stays off the owned track.
                if matches!(obj_ty, Type::Str | Type::Substr) && vt.is_refcounted() {
                    ctx.owned_member_reads.insert(eid);
                }
                boxed
            }
        }
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(elem, Operand::Value(res_slot), 0),
    );
    let hb = ctx.cur_block;
    ctx.f.set_term(hb, Terminator::Br(after));
    ctx.cur_block = after;
    let r = ctx.f.append_inst(
        after,
        InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
        Type::Any,
        None,
    );
    Operand::Value(r)
}

/// Nullish test for the receiver: Any → tag ∈ {ANY_NULL=0,
/// ANY_UNDEF=5}; ptr-typed → `recv == null` (statically non-null
/// receivers constant-fold the branch away). Shared with the
/// OptCall lowering (chunk 705).
pub(crate) fn emit_nullish_cond(
    ctx: &mut LowerCtx<'_>,
    obj_op: &Operand,
    obj_ty: &Type,
) -> Operand {
    if matches!(obj_ty, Type::Any) {
        let tag = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_unbox_tag, vec![obj_op.clone()]),
            Type::I64,
            None,
        );
        let is_null = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let is_undef = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5)),
            Type::Bool,
            None,
        );
        let nullish = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                SsaBinOp::Or,
                Operand::Value(is_null),
                Operand::Value(is_undef),
            ),
            Type::Bool,
            None,
        );
        Operand::Value(nullish)
    } else {
        let cond = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Eq, obj_op.clone(), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        Operand::Value(cond)
    }
}

/// Any-receiver hit path — mirrors `ssa_lower_index::lower_from_value`'s
/// Any dispatch: a compile-time string-literal key rides the full
/// member-read path; a dynamic String-typed key probes by its runtime
/// Str cell; everything else is a numeric element read through
/// `any_index_get` (the receiver is guarded non-nullish here, so the
/// helper's own nullish TypeError path is dead).
fn lower_any_hit(ctx: &mut LowerCtx<'_>, eid: ExprId, obj_op: Operand, index: ExprId) -> Operand {
    if let crate::ast::Expr::String(lit) = ctx.ast.get_expr(index) {
        let lit = lit.clone();
        return crate::ssa_lower_any_member::lower_any_member_read(ctx, eid, obj_op, &lit);
    }
    // Chunk 726 — both dynamic-key lanes answer a fresh owned box
    // (the runtime getters take +1 on heap payloads); the eid joins
    // the owned track so the consumer's release balances (probes
    // q726f numeric / q726g str-key: fresh-element churn 15.3MB vs
    // 6.2MB flat). The string-literal lane above records inside
    // `lower_any_member_read` (chunk 717).
    if matches!(ctx.expr_types.get(&index), Some(crate::check::Type::String)) {
        let v = crate::ssa_lower_index::lower_any_index_str_key(ctx, obj_op, index);
        ctx.owned_member_reads.insert(eid);
        return v;
    }
    let idx_val = ctx.lower_index_operand(index);
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.any_index_get, vec![obj_op, idx_val]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    ctx.owned_member_reads.insert(eid);
    Operand::Value(v)
}
