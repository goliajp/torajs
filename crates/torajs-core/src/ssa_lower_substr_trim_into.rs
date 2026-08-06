//! Step 13-a — `<Substr>.trim()` stack-write fast-path.
//!
//! Bench-driven A2 path (b) ladder, hotified from Step 12 close-out:
//! csv-trim-100k at 1.059× vs rust (largest single drag on the
//! vs-rust geomean, MISSING the A2 gate ≥1.60× by 0.012×). IR
//! survey at HEAD `8605add` identified `__torajs_substr_trim`'s
//! ephemeral `SubstrBlock` allocation (live for 4 IR insts: create
//! → load .len → drop) as ~38% of csv-trim-100k's runtime.
//!
//! Fix-design Option A (docs/v0.7-A2-finding.md § "Step 13"):
//! introduce `__torajs_substr_trim_into(v, out_buf)` (Rust impl in
//! `torajs-str/src/substr_methods.rs`) that writes the trimmed view
//! into a caller-provided 32-byte stack buffer, skipping the
//! pool_pop / pool_push roundtrip in
//! `__torajs_substr_create` / `drop_pool_aware`'s non-INLINE branch.
//! The buffer carries `FLAG_SUBSTR_INLINE` so the auto-emitted
//! `__torajs_substr_drop(buf)` at scope end follows the INLINE
//! branch (dec parent rc, return — no pool/free).
//!
//! Lives in its own file to keep `ssa_lower.rs` on its known-debt
//! shrink-only trajectory (`.claude/rules/common/file-size.md`).
//! Net ssa_lower.rs delta is offset by folding the existing
//! `__torajs_substr_trim` / `_trim_start` / `_trim_end`
//! declare_intrinsic 7-line blocks into [`declare_all`] so the
//! per-trim-variant declare boilerplate moves out of
//! `ssa_lower.rs` entirely.

use std::collections::HashMap;

use crate::ssa::{FuncId, InstKind, Module, Operand, Type};
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
/// byte caller stack buffer, calls `__torajs_substr_trim_into(recv,
/// buf)`, and returns `buf` as the resulting Substr value. The
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
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.substr_trim_into,
            vec![recv_op, Operand::Value(buf)],
        ),
    );
    Some(Operand::Value(buf))
}
