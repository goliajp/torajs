//! Public compile entry points + the LLVM-codegen serialization
//! mutex.
//!
//! All four entry fns funnel into `compile_for_kind_impl` in
//! `ssa_inkwell.rs`, optionally serving a cached fixture .o on
//! the way. `COMPILE_LOCK` serializes every codegen invocation
//! because LLVM holds non-thread-safe global state (target /
//! pass registration, command-line option parsing, internal
//! statistics); two parallel compiles race those globals and
//! SIGSEGV/SIGBUS intermittently. `Mutex::new` is const since
//! Rust 1.63 so this is a true zero-init static.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25,
//! batch 8).

use std::path::{Path, PathBuf};

use crate::ssa::Module;

use super::link::{link_object_to_binary, rand_suffix, read_uses_fetch_sidecar};
use super::{COMPILE_LOCK, CompileError, CompileTarget, OutputKind, compile_for_kind_impl};

/// Compile an SSA module to a native binary at `out_path`. `opt` selects the
/// LLVM new-pass-manager pipeline ("O0" / "O1" / "O2" / "O3"); the default
/// is "O1" because that's the bench-tuned setting for fib40.
///
/// `source_path` (v0.3 #4 D-2) — when supplied, emits DWARF
/// debug-info: a DICompileUnit + DIFile pinned to the .ts source,
/// and per-fn DISubprogram so backtrace tools (atos, addr2line) see
/// `tr` fns as proper named scopes. D-3 plumbs per-instruction
/// DILocation; D-4 wires runtime panic backtraces into this.
pub fn compile(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
) -> Result<(), CompileError> {
    compile_for(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        CompileTarget::Native,
    )
}

pub fn compile_for(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
) -> Result<(), CompileError> {
    compile_for_kind(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        OutputKind::Executable,
    )
}

/// V3-16 — extended entry point that lets the caller pick
/// executable vs shared-lib output. `compile_for` keeps the
/// existing executable-only signature so existing callers
/// (`tr build`, `tr run`, bench harness) don't need to thread
/// the new param.
pub fn compile_for_kind(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
    kind: OutputKind,
) -> Result<(), CompileError> {
    compile_for_kind_with_cache(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        kind,
        None,
    )
}

/// B-1 phase 2 — variant of `compile_for_kind` that takes an
/// optional path to a cached per-fixture `.o` file.
///
/// Fast path (cache hit): copy cached `.o` → temp, skip the LLVM
/// pipeline entirely (parse/check/lower happened upstream in the
/// caller; the cached .o is byte-identical to what LLVM would emit
/// for the same source against the same compiler-rev). Read the
/// `.uses_fetch` sidecar to decide whether to add `-lcurl`. Jump
/// straight to runtime cc + final link.
///
/// Slow path (cache miss, or cache None): runs the full LLVM compile
/// as before. On miss with a cache slot provided, also copies the
/// freshly produced `.o` + uses_fetch sidecar into the cache slot
/// (atomically) for future hits.
///
/// `fixture_o_cache` key contract: caller MUST ensure the same source
/// + opt + compiler_rev produces the same cache path (see
/// `TORAJS_COMPILER_REV` in build.rs). Mismatch = silent stale .o.
#[allow(clippy::too_many_arguments)]
pub fn compile_for_kind_with_cache(
    ssa_module: &Module,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    ast: Option<&crate::ast::Ast>,
    target: CompileTarget,
    kind: OutputKind,
    fixture_o_cache: Option<&Path>,
) -> Result<(), CompileError> {
    // Fast path: cached fixture .o exists → skip the entire LLVM
    // pipeline, jump straight to link.
    if let Some(cache_p) = fixture_o_cache
        && cache_p.is_file()
    {
        let _guard = COMPILE_LOCK
            .lock()
            .expect("ssa_inkwell COMPILE_LOCK poisoned by a prior panicking compile");
        let pid = std::process::id();
        let obj_path: PathBuf =
            std::env::temp_dir().join(format!("torajs-llvm-{}-{}.o", pid, rand_suffix()));
        std::fs::copy(cache_p, &obj_path)
            .map_err(|e| CompileError::Link(format!("copy cached fixture .o: {e}")))?;
        let uses_fetch = read_uses_fetch_sidecar(cache_p);
        let result = link_object_to_binary(
            &obj_path,
            out_path,
            opt,
            source_path,
            target,
            kind,
            uses_fetch,
        );
        let _ = std::fs::remove_file(&obj_path);
        return result;
    }

    // Slow path: full LLVM compile.
    compile_for_kind_impl(
        ssa_module,
        out_path,
        opt,
        source_path,
        ast,
        target,
        kind,
        fixture_o_cache,
    )
}
