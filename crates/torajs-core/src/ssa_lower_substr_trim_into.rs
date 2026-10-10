//! Step 13-a — `<Substr>.trim()` stack-write fast-path.
//!
//! Bench-driven A2 path (b) ladder, hotified from Step 12 close-out:
//! csv-trim-100k at 1.059× vs rust (largest single drag on the
//! vs-rust geomean, MISSING the A2 gate ≥1.60× by 0.012×). IR
//! survey at HEAD `8605add` identified `__torajs_substr_trim`'s
//! ephemeral `SubstrBlock` allocation (live for 4 IR insts: create
//! → load .len → drop) as ~38% of csv-trim-100k's runtime.
//!
//! Fix-design Option A: introduce `__torajs_substr_trim_into(v,
//! out_buf)` (Rust impl in
//! `torajs-str/src/substr_methods.rs`) that writes the trimmed view
//! into a caller-provided 32-byte stack buffer, skipping the
//! pool_pop / pool_push roundtrip in
//! `__torajs_substr_create` / `drop_pool_aware`'s non-INLINE branch.
//! The buffer carries `FLAG_SUBSTR_INLINE` so the auto-emitted
//! `__torajs_substr_drop(buf)` at scope end follows the INLINE
//! branch (dec parent rc, return — no pool/free).
//!
//! Lives in its own file to keep `ssa_lower.rs` on its known-debt
//! shrink-only trajectory.
//! Net ssa_lower.rs delta is offset by folding the existing
//! `__torajs_substr_trim` / `_trim_start` / `_trim_end`
//! declare_intrinsic 7-line blocks into [`declare_all`] so the
//! per-trim-variant declare boilerplate moves out of
//! `ssa_lower.rs` entirely.

mod scan;

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::ssa::{BinOp, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

/// Declare all 4 trim-variant intrinsics on `module` + `fn_table`
/// in one shot:
///
/// - `__torajs_substr_trim`        — heap-alloc (existing, kept for
///   escape paths where the result outlives the receiver).
/// - `__torajs_substr_trim_start`  — heap-alloc (existing).
/// - `__torajs_substr_trim_end`    — heap-alloc (existing).
/// - `__torajs_substr_trim_into`   — Step 13-a stack-write
///   (`(*const Substr, *mut [u8; 32]) -> void`).
///
/// Returns `(trim_id, trim_start_id, trim_end_id, trim_into_id)`.
/// Folds the previously-repeated declare_intrinsic 7-line blocks
/// in `ssa_lower.rs` so adding the new trim_into entry point nets
/// zero growth on ssa_lower.rs.
pub(crate) fn declare_all(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> (FuncId, FuncId, FuncId, FuncId) {
    let trim = declare_intrinsic(
        module,
        fn_table,
        "__torajs_substr_trim",
        &[Type::Ptr],
        Type::Substr,
    );
    let trim_start = declare_intrinsic(
        module,
        fn_table,
        "__torajs_substr_trim_start",
        &[Type::Ptr],
        Type::Substr,
    );
    let trim_end = declare_intrinsic(
        module,
        fn_table,
        "__torajs_substr_trim_end",
        &[Type::Ptr],
        Type::Substr,
    );
    let trim_into = declare_intrinsic(
        module,
        fn_table,
        "__torajs_substr_trim_into",
        &[Type::Ptr, Type::Ptr],
        Type::Void,
    );
    (trim, trim_start, trim_end, trim_into)
}

/// Try emitting the stack-write fast-path for `<recv>.trim()` when
/// `recv` is a `Type::Substr` and `args` is empty. Allocates a 32-
/// byte caller stack buffer, fills it with the trimmed view (inline
/// scan for a Latin-1 parent, `__torajs_substr_trim_into(recv, buf)`
/// otherwise), and returns `buf` as the resulting Substr value. The
/// auto-emitted scope-end `__torajs_substr_drop(buf)` will follow
/// the INLINE branch (dec parent rc, no pool push, no free).
///
/// Returns `None` (caller falls through to the regular view-aware
/// dispatch which emits `__torajs_substr_trim`) when:
/// - `method != "trim"`, OR
/// - `args_len != 0`, OR
/// - `recv_ty != Type::Substr`.
///
/// `trimStart` / `trimEnd` are not yet handled — Step 13-b will
/// mirror this pattern for them once 13-a's bench delta is acked.
pub(crate) fn try_emit(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args_len: usize,
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if recv_ty != Type::Substr || method != "trim" || args_len != 0 {
        return None;
    }
    // The buffer IS the trimmed view, so the value carries
    // `Type::Substr` and not the raw `Type::Ptr` the alloca would
    // otherwise answer with. Two things ride on that type, and both
    // were broken while it read `Ptr`:
    //
    // - consumers dispatch on it. `.length` has a Str/Substr arm and
    //   nothing for a bare pointer, so `parts[i].trim().length`
    //   reached `ssa-lower: member access on non-object Ptr` — a
    //   compile error on a shape that had built before.
    // - the drop is emitted from it. `__torajs_substr_trim_into`
    //   rc_incs the parent and stamps `FLAG_SUBSTR_INLINE` expressly
    //   so the matching `__torajs_substr_drop` takes the inline
    //   branch — dec the parent, no pool push, no free, which is what
    //   makes a stack buffer safe to drop. Typed `Ptr` no drop was
    //   ever emitted, so that inc leaked.
    let buf = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::AllocaBytes(32), Type::Substr, None);
    scan::emit(ctx, recv_op, scan::Out::View(buf));
    Some(Operand::Value(buf))
}

/// `<recv>.trim()` on a view whose product the census proved is only
/// ever measured (`ast::trim_length_only`): the trimmed length as an
/// `I64`, with no view written. `None` for any other call, which then
/// takes [`try_emit`] or the general dispatch.
pub(crate) fn try_emit_length(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    method: &str,
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if recv_ty != Type::Substr
        || method != "trim"
        || !ctx.ast.trim_length_only_calls.contains(&call_eid)
    {
        return None;
    }
    let entry = ctx.cur_block;
    let slot = ctx
        .f
        .append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, None);
    scan::emit(ctx, recv_op, scan::Out::Length(slot));
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    );
    Some(Operand::Value(len))
}

/// A measured `part.trim()` on a versioned split loop's variable: the
/// length from the loop's own registers, the part's offset and length
/// over the loop-invariant payload base, with no Substr read. Only the
/// UTF-16 arm, which always fills the loop's Substr, asks the kernel.
/// The loop leaves that Substr unfilled on its Latin-1 arm only when
/// every read of the variable is one of these
/// (`ast::sfi_safe::split_var_view_reads_only`), so this must answer
/// whenever that census does: a census-listed call on the variable.
pub(crate) fn try_split_part_length(
    ctx: &mut LowerCtx<'_>,
    call_eid: ExprId,
    recv: ExprId,
) -> Option<Operand> {
    if !is_measured_trim(ctx, call_eid) {
        return None;
    }
    let view = crate::ssa_lower_stmt_for_of_split_iter::view_of(ctx, recv)?;
    let cur = ctx.cur_block;
    let slot = ctx
        .f
        .append_inst(cur, InstKind::Alloca(Type::I64), Type::Ptr, None);
    let lat = ctx.f.add_block();
    let wide = ctx.f.add_block();
    let join = ctx.f.add_block();
    ctx.f.set_term(
        cur,
        Terminator::CondBr {
            cond: Operand::Value(view.is_lat),
            then_blk: lat,
            else_blk: wide,
        },
    );

    let i64_load = |ctx: &mut LowerCtx<'_>, at, slot| {
        ctx.f.append_inst(
            at,
            InstKind::Load(Type::I64, Operand::Value(slot), 0),
            Type::I64,
            None,
        )
    };
    let off = i64_load(ctx, lat, view.off_slot);
    let len = i64_load(ctx, lat, view.len_slot);
    let p = ctx.f.append_inst(
        lat,
        InstKind::PtrToInt(Operand::Value(view.pbase)),
        Type::I64,
        None,
    );
    let start = ctx.f.append_inst(
        lat,
        InstKind::BinOp(BinOp::Add, Operand::Value(p), Operand::Value(off)),
        Type::I64,
        None,
    );
    let base = ctx.f.append_inst(
        lat,
        InstKind::IntToPtr(Operand::Value(start)),
        Type::Ptr,
        None,
    );
    ctx.cur_block = lat;
    scan::emit_latin1_length(ctx, base, len, slot);
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join));

    let part = ctx.f.append_inst(
        wide,
        InstKind::Load(Type::Substr, Operand::Value(view.v_slot), 0),
        Type::Substr,
        None,
    );
    scan::emit_kernel(ctx, wide, Operand::Value(part), scan::Out::Length(slot));
    ctx.f.set_term(wide, Terminator::Br(join));

    ctx.cur_block = join;
    let n = i64_load(ctx, join, slot);
    Some(Operand::Value(n))
}

/// `eid` is a trim call the census listed; it arrives as its length
/// when [`try_emit_length`] took it, which the `I64` type tells.
pub(crate) fn is_measured_trim(ctx: &LowerCtx<'_>, eid: ExprId) -> bool {
    ctx.ast.trim_length_only_calls.contains(&eid)
}

/// `obj`, lowered to an operand of type `obj_ty`, is a measured trim
/// product: the call itself, or a binding that holds one — a name the
/// checker types a string whose slot holds an `I64`, which only
/// `finalize_and_bind` makes, and only for this lane.
pub(crate) fn operand_is_trim_length(ctx: &LowerCtx<'_>, obj: ExprId, obj_ty: &Type) -> bool {
    if *obj_ty != Type::I64 {
        return false;
    }
    match ctx.ast.get_expr(obj) {
        crate::ast::Expr::Ident(n) => {
            matches!(ctx.expr_types.get(&obj), Some(crate::check::Type::String))
                && ctx.locals.get(n).is_some_and(|l| l.ty == Type::I64)
        }
        _ => is_measured_trim(ctx, obj),
    }
}
