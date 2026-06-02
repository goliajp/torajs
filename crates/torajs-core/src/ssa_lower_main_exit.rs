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

use crate::ssa::{FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

const MAIN_EXIT_CODE_SYM: &str = "__torajs_main_exit_code";

/// Declare the `__torajs_main_exit_code` extern at module setup.
/// Caller passes the module + fn_table being built; we register
/// the symbol and discard the returned FuncId (it's reachable via
/// `fn_table[MAIN_EXIT_CODE_SYM]` at `emit_ret` time).
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    declare_intrinsic(module, fn_table, MAIN_EXIT_CODE_SYM, &[], Type::I32);
}

/// Emit the synthesized-main exit sequence — `call
/// __torajs_main_exit_code; ret <i32>` — as the terminator of
/// `ctx.cur_block`. Called from `synthesize_main` immediately
/// after the microtask drain + cycle-collector drain.
pub(crate) fn emit_ret(ctx: &mut LowerCtx) {
    let fid = *ctx
        .fn_table
        .get(MAIN_EXIT_CODE_SYM)
        .expect("__torajs_main_exit_code must be declared before main emit");
    let exit_code = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::Call(fid, vec![]), Type::I32, None);
    let cb = ctx.cur_block;
    ctx.f
        .set_term(cb, Terminator::Ret(Some(Operand::Value(exit_code))));
}
