//! Pass 0 `declare_intrinsic` group: process surface (v0.3 #3 +
//! #3.c argv/envp plumbing).
//!
//! chunk 119 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-118). 6 declarations covering:
//!
//! - `process_exit(code) -> Void` — `process.exit()`
//! - `process_cwd() -> Str` — `process.cwd()`
//! - `process_platform() -> Str` — `process.platform`
//! - `process_getenv(name) -> Str` — `process.env[name]`
//! - `argv_init(argc, argv, envp) -> Void` — called once at the start
//!   of main with the LLVM-widened argc/argv/envp params; stores them
//!   into runtime globals. envp is null on WASI
//!   (`__main_argc_argv` is 2-param); the entry-block wrapper
//!   forwards a const-null ptr in that case so the call site stays
//!   uniform.
//! - `process_argv() -> Ptr` — returns Array<Str> built from the
//!   captured globals. Called by `process.argv` / `Bun.argv`.

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct ProcessIds {
    pub process_exit: FuncId,
    pub process_cwd: FuncId,
    pub process_platform: FuncId,
    pub process_getenv: FuncId,
    pub argv_init: FuncId,
    pub process_argv: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> ProcessIds {
    ProcessIds {
        process_exit: declare_intrinsic(
            module,
            fn_table,
            "__torajs_process_exit",
            &[Type::I64],
            Type::Void,
        ),
        process_cwd: declare_intrinsic(module, fn_table, "__torajs_process_cwd", &[], Type::Str),
        process_platform: declare_intrinsic(
            module,
            fn_table,
            "__torajs_process_platform",
            &[],
            Type::Str,
        ),
        process_getenv: declare_intrinsic(
            module,
            fn_table,
            "__torajs_process_getenv",
            &[Type::Str],
            Type::Str,
        ),
        argv_init: declare_intrinsic(
            module,
            fn_table,
            "__torajs_argv_init",
            &[Type::I32, Type::Ptr, Type::Ptr],
            Type::Void,
        ),
        process_argv: declare_intrinsic(module, fn_table, "__torajs_process_argv", &[], Type::Ptr),
    }
}
