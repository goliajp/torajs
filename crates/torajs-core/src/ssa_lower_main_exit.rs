//! P10.5-A3-b — `main`'s exit-code wire-up.
//!
//! Two thin helpers that keep the bulk of the unhandled-rejection
//! exit-code path out of `ssa_lower.rs` (file-size known-debt #1).
//!
//! - [`declare`] registers the `__torajs_main_exit_code() -> i32`
//!   runtime intrinsic in the module-setup phase. The FuncId is
//!   stashed in `fn_table` under the symbol name so [`emit_ret`]
//!   can look it up later without an `Intrinsics` struct field —
//!   keeps the surface change in `ssa_lower.rs` to a single
//!   call-site at each end.
//! - [`emit_ret`] replaces the prior hard-coded
//!   `set_term(Ret(ConstI32(0)))` at the end of
//!   `synthesize_main`. Emits `call __torajs_main_exit_code`
//!   followed by `ret <result>` so the synthesized `main` exits
//!   with `0` on a clean run and `1` if the
//!   `torajs_promise::unhandled` reporter fired during the
//!   microtask drain.
//!
//! Runtime semantics live in
//! `torajs-promise::unhandled::__torajs_main_exit_code`.

use std::collections::HashMap;

use crate::ssa::{FuncId, IPred, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

const MAIN_EXIT_CODE_SYM: &str = "__torajs_main_exit_code";

/// bug-327 C2.5 — uncaught-throw reporter + exit code. Called from
/// `emit_throw_check`'s main-frame propagate branch (a throw that
/// escaped every user frame): reports the pending throw to stderr
/// and returns 1. Runtime lives in torajs-throw.
const UNCAUGHT_EXIT_CODE_SYM: &str = "__torajs_uncaught_exit_code";

/// Declare the `__torajs_main_exit_code` /
/// `__torajs_uncaught_exit_code` externs at module setup. Caller
/// passes the module + fn_table being built; we register the
/// symbols and discard the returned FuncIds (reachable via
/// `fn_table[<sym>]` at emit time).
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(module, fn_table, MAIN_EXIT_CODE_SYM, &[], Type::I32);
    declare_intrinsic(module, fn_table, UNCAUGHT_EXIT_CODE_SYM, &[], Type::I32);
}

/// Emit `call __torajs_main_exit_code` and answer the exit-code
/// value. Called from `synthesize_main` right after the microtask
/// drain and BEFORE the top-level teardown (locals / globals drops +
/// cycle drain): the sweep inside fires the unhandled-rejection
/// reporter, which walks a rejected reason's prototype chain, and a
/// class instance's proto edge is borrowed from the class registry —
/// sweeping after the registry stakes were dropped read freed proto
/// cells (the rotation-348 promise-pool UAF / rc-underflow family).
pub(crate) fn emit_exit_code(ctx: &mut LowerCtx) -> crate::ssa::ValueId {
    let fid = *ctx
        .fn_table
        .get(MAIN_EXIT_CODE_SYM)
        .expect("__torajs_main_exit_code must be declared before main emit");
    ctx.f
        .append_inst(ctx.cur_block, InstKind::Call(fid, vec![]), Type::I32, None)
}

/// Terminate the synthesized main: `ret <exit_code>`. Runs after the
/// teardown sequence, returning the value [`emit_exit_code`] captured
/// while the heap was still fully alive.
pub(crate) fn emit_ret(ctx: &mut LowerCtx, exit_code: crate::ssa::ValueId) {
    let cb = ctx.cur_block;
    ctx.f
        .set_term(cb, Terminator::Ret(Some(Operand::Value(exit_code))));
}

/// r505 (A12) — release the class object / prototype cells at exit
/// through the by-tag registry. The synthesized prologue
/// (`crate::ast::CLASS_PROLOGUE_FN`) mints the cells and hands its
/// +1 to the registry rather than dropping them at its own return,
/// so main owns the release: for every class tag, read the raw
/// registry bits (`class_cell_raw` / `proto_cell_raw` — a borrow, no
/// rc traffic, and NOT a guard reader, exactly like the prologue's
/// writeback reads) and release only when the slot is non-zero. Zero
/// is the kernel's own "never registered" — a class without a
/// register call, a generic specialization tag aliasing its main
/// slot, or the prologue assumed away by the link judgment — and
/// then there is nothing to release. Class cell first, prototype
/// cell second: the reverse of the prologue's mint order, the order
/// main's scope drop used when the cells were its locals.
pub(crate) fn emit_class_cell_registry_release(ctx: &mut LowerCtx) {
    let mut tags: Vec<u32> = ctx.class_name_to_tag.values().copied().collect();
    tags.sort_unstable();
    tags.dedup();
    for tag in tags {
        for raw_fid in [ctx.intrinsics.class_cell_raw, ctx.intrinsics.proto_cell_raw] {
            let raw = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(raw_fid, vec![Operand::ConstI64(tag as i64)]),
                Type::I64,
                None,
            );
            let registered = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(raw), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let release_blk = ctx.f.add_block();
            let after = ctx.f.add_block();
            ctx.f.set_term(
                ctx.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(registered),
                    then_blk: release_blk,
                    else_blk: after,
                },
            );
            let cell = ctx.f.append_inst(
                release_blk,
                InstKind::IntToPtr(Operand::Value(raw)),
                Type::Any,
                None,
            );
            ctx.f.append_void(
                release_blk,
                InstKind::Call(
                    ctx.intrinsics.class_cell_release,
                    vec![Operand::Value(cell)],
                ),
            );
            ctx.f.set_term(release_blk, Terminator::Br(after));
            ctx.cur_block = after;
        }
    }
}
