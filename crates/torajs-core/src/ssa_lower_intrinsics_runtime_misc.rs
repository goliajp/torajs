//! Pass 0 `declare_intrinsic` group: cycle collector + Symbol +
//! sync stdio + microtask drain.
//!
//! chunk 127 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-126). 12 declarations covering four small contiguous
//! runtime substrate groups that lived between map/set and the
//! Promise family inline at the SSA layer.
//!
//! The `gc` global-fn alias (`fn_table.insert("gc", cycle_collect_id)`)
//! stays in the caller — it depends on the FuncId returned here. The
//! inline `ssa_lower_main_exit::declare` / `ssa_lower_process_on::declare`
//! calls that sit between `microtask_drain` and the queueMicrotask
//! enqueue intrinsics also stay inline at the caller; they declare
//! their own intrinsics in dedicated siblings already.
//!
//! Subgroups:
//! - **Bacon-Rajan cycle collector** (T-26.C, 3 ids):
//!   `cycle_buffer(p)` hot-path call from the inline Obj drop's
//!   else-branch when rc stays positive; `cycle_collect()` is the
//!   manual `gc()` trigger (mark/scan/collect over buffered roots);
//!   `cycle_at_exit_drain()` is the main-exit drain (synthesize_main
//!   emits it as the last step before Ret so cycle roots accumulated
//!   during program lifetime are freed before process exit; same
//!   body as cycle_collect today, kept as a separate symbol for
//!   independent policy evolution).
//! - **Symbol value runtime** (T-13.a/b/c, 6 ids): alloc(desc) /
//!   drop / print (`Symbol(<desc>)` console.log form);
//!   `Symbol.for(key)` global registry + `keyFor(sym)` reverse
//!   lookup; lazy-init well-known singletons
//!   `Symbol.iterator` / `Symbol.asyncIterator` /
//!   `Symbol.toPrimitive` (each rc-inc's for the caller on call).
//! - **Sync stdio** (T-03 v0.3, 2 ids): `process.stdout.write(s)` +
//!   `process.stderr.write(s)` return bytes-written (i64 — sig
//!   currently Bool, see runtime). Aborts on short-write per runtime
//!   helper docstring. `process.stdin.read()` deferred to v0.5 async.
//! - **Microtask queue drain** (v0.5 T-15.e, 1 id):
//!   `microtask_run_until_idle` is auto-called at the end of main so
//!   Promise callbacks chained via `.then` before exit get a chance
//!   to run. No-op when queue empty → non-async programs pay one
//!   fn-call worth of overhead at exit.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct RuntimeMiscIds {
    pub cycle_buffer: FuncId,
    pub cycle_collect: FuncId,
    pub cycle_at_exit_drain: FuncId,
    pub symbol_alloc: FuncId,
    pub symbol_drop: FuncId,
    pub symbol_print: FuncId,
    pub symbol_for: FuncId,
    pub symbol_key_for: FuncId,
    pub symbol_for_any: FuncId,
    pub symbol_key_for_any: FuncId,
    pub symbol_iterator: FuncId,
    pub symbol_async_iterator: FuncId,
    pub symbol_to_primitive: FuncId,
    pub symbol_well_known: FuncId,
    pub process_stdout_write: FuncId,
    pub process_stderr_write: FuncId,
    pub microtask_drain: FuncId,
    /// RFC 20260804-fnprops-canonical-cell — binds a fn ptr's props
    /// storage to its canonical forward cell at the cell's lazy mint
    /// (bag migrates; both spellings share one slot after).
    pub fnprops_bind_cell: FuncId,
    /// §13.4.4.1 ToNumeric + step over a VALUE (no slot) — the
    /// any-member update lane composes it between its GetV and the
    /// member-set kernel. Writes the coerced old value through the
    /// ptr arg, answers the stepped new value.
    pub anyv_incr_value: FuncId,
}

pub(crate) fn declare(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
) -> RuntimeMiscIds {
    RuntimeMiscIds {
        cycle_buffer: declare_intrinsic(
            module,
            fn_table,
            "__torajs_cycle_buffer",
            &[Type::Ptr],
            Type::Void,
        ),
        cycle_collect: declare_intrinsic(
            module,
            fn_table,
            "__torajs_cycle_collect",
            &[],
            Type::Void,
        ),
        cycle_at_exit_drain: declare_intrinsic(
            module,
            fn_table,
            "__torajs_cycle_at_exit_drain",
            &[],
            Type::Void,
        ),
        symbol_alloc: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_alloc",
            &[Type::Str],
            Type::Symbol,
        ),
        symbol_drop: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_drop",
            &[Type::Symbol],
            Type::Void,
        ),
        symbol_print: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_print",
            &[Type::Symbol],
            Type::Void,
        ),
        symbol_for: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_for",
            &[Type::Str],
            Type::Symbol,
        ),
        symbol_key_for: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_key_for",
            &[Type::Symbol],
            Type::Str,
        ),
        symbol_for_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_for_any",
            &[Type::Any],
            Type::Any,
        ),
        symbol_key_for_any: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_key_for_any",
            &[Type::Any],
            Type::Any,
        ),
        symbol_iterator: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_iterator",
            &[],
            Type::Symbol,
        ),
        symbol_async_iterator: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_async_iterator",
            &[],
            Type::Symbol,
        ),
        symbol_to_primitive: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_to_primitive",
            &[],
            Type::Symbol,
        ),
        symbol_well_known: declare_intrinsic(
            module,
            fn_table,
            "__torajs_symbol_well_known",
            &[Type::I64],
            Type::Symbol,
        ),
        process_stdout_write: declare_intrinsic(
            module,
            fn_table,
            "__torajs_process_stdout_write",
            &[Type::Str],
            Type::Bool,
        ),
        process_stderr_write: declare_intrinsic(
            module,
            fn_table,
            "__torajs_process_stderr_write",
            &[Type::Str],
            Type::Bool,
        ),
        microtask_drain: declare_intrinsic(
            module,
            fn_table,
            "__torajs_microtask_run_until_idle",
            &[],
            Type::Void,
        ),
        fnprops_bind_cell: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fnprops_bind_cell",
            &[Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        anyv_incr_value: declare_intrinsic(
            module,
            fn_table,
            "__torajs_anyv_incr_value",
            &[Type::Any, Type::I64, Type::Ptr],
            Type::Any,
        ),
    }
}
