//! Post-IR-emission compile pipeline — LLVM verify, target-machine
//! creation, pass-manager run, object-file emit, optional cache
//! write, then handoff to `link_object_to_binary`.
//!
//! Extracted from `compile_for_kind_impl` in `ssa_inkwell.rs`
//! (2026-05-25, god-file decomp batch 22a).

use std::path::{Path, PathBuf};

use inkwell::OptimizationLevel;
use inkwell::module::Module as LlvmModule;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};

use super::link::{link_object_to_binary, rand_suffix, write_uses_fetch_sidecar};
use super::{CompileError, CompileTarget, OutputKind};

/// Run the LLVM verify / optimize / emit / link tail of
/// `compile_for_kind_impl`. Takes the already-populated module +
/// caller's target/opt/output choices; ignores `ssa_module` after
/// `uses_fetch` has been computed by the caller.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_and_link(
    llvm_module: &LlvmModule,
    opt: &str,
    source_path: Option<&Path>,
    target: CompileTarget,
    kind: OutputKind,
    out_path: &Path,
    fixture_o_cache: Option<&Path>,
    uses_fetch: bool,
) -> Result<(), CompileError> {
    if let Err(e) = llvm_module.verify() {
        return Err(CompileError::Verify(e.to_string()));
    }

    let (triple, cpu, features) = match target {
        CompileTarget::Native => {
            Target::initialize_aarch64(&InitializationConfig::default());
            (
                TargetMachine::get_default_triple(),
                TargetMachine::get_host_cpu_name().to_string(),
                TargetMachine::get_host_cpu_features().to_string(),
            )
        }
        CompileTarget::Wasm32Wasi => {
            // T-20 (v0.6.0) — initialize the WebAssembly backend in
            // LLVM 22. wasm32-wasip1 is the canonical target triple
            // (LLVM 22 deprecated the older "wasm32-wasi" spelling).
            // No cpu / feature tuning — the default subset works on
            // every wasm engine.
            Target::initialize_webassembly(&InitializationConfig::default());
            (
                inkwell::targets::TargetTriple::create("wasm32-wasip1"),
                String::new(),
                String::new(),
            )
        }
    };
    let target_obj = Target::from_triple(&triple).map_err(|e| CompileError::Emit(e.to_string()))?;
    /* Codegen optimization level. NOT bumped to Aggressive: empirically
     * measured a net -1.5% geomean regression at OptLevel::Aggressive
     * (2026-05-22 / P-PERF.A2 attempt: gcd1m / generic-id / mandelbrot
     * +1–6% but async-fn-call +14%, promise-all +11%, startup +4.7%
     * regressed past noise. Net-negative on Promise/closure allocation
     * patterns — Aggressive's register-pressure/peephole changes hurt
     * the alloc-heavy paths more than they help the pure-numeric ones).
     * Keep Less; the IR pipeline runs at `default<O3>` (per `opt`
     * above) which is where the bulk of optimization lives. */
    /* Reloc mode stays at PIC. A P-PERF.A5 attempt switched native
     * Executable to Static (2026-05-22, reverted same day): hoped
     * to elide GOT indirection on cross-TU calls, but the bench
     * cycle ran on a thermal-loaded machine and showed correlated
     * tora-and-bun regression of 5–15 % across most cases (high
     * shared noise; couldn't isolate the Static-vs-PIC signal
     * cleanly). geomean vs bun-aot 4.155 → 4.145, vs node-v8 20.86
     * → 19.99 — both within noise and not a clear improvement
     * direction. Keeping PIC until a quiescent-machine rerun can
     * give a cleaner measurement, or until PGO arrives and re-
     * justifies the reloc question. Archived bench evidence at
     * bench/results/2026-05-22-mini-3bf6002.json. */
    let machine = target_obj
        .create_target_machine(
            &triple,
            &cpu,
            &features,
            OptimizationLevel::Less,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| CompileError::Emit("create_target_machine returned None".into()))?;
    // Pin the module's triple + datalayout for non-native targets so
    // the WebAssembly verifier sees a matching ABI. Native target
    // intentionally skips this — LLVM's implicit host detection picks
    // the right datalayout AND keeps a faster optimization path that
    // an explicit `set_data_layout` (even with the same string)
    // disables. Measured: explicitly setting on native costs ~17% on
    // the bench geomean (T-20.a regression that only surfaced at the
    // v0.6.0 perf gate). wasm always needs the explicit set or the
    // verifier rejects mismatched host-vs-target datalayout.
    if matches!(target, CompileTarget::Wasm32Wasi) {
        llvm_module.set_triple(&triple);
        llvm_module.set_data_layout(&machine.get_target_data().get_data_layout());
    }

    let pipeline = format!("default<{opt}>");
    llvm_module
        .run_passes(&pipeline, &machine, PassBuilderOptions::create())
        .map_err(|e| CompileError::Pass(format!("{pipeline}: {}", e.to_string())))?;

    let obj_path: PathBuf = std::env::temp_dir().join(format!(
        "torajs-llvm-{}-{}.o",
        std::process::id(),
        rand_suffix()
    ));
    machine
        .write_to_file(llvm_module, FileType::Object, &obj_path)
        .map_err(|e| CompileError::Emit(e.to_string()))?;
    /* T-20.b debug — when env var set, also dump LLVM IR + .o
     * copy for postmortem of wasm signature errors. */
    if std::env::var("TR_DEBUG_KEEP").is_ok() {
        let _ = std::fs::write(
            "/tmp/torajs-debug.ll",
            llvm_module.print_to_string().to_string(),
        );
        let _ = std::fs::copy(&obj_path, "/tmp/torajs-debug.o");
    }

    // B-1 phase 2 — cache write: copy the freshly produced .o + write
    // a uses_fetch sidecar so future hits can rebuild the link command
    // without scanning the SSA. Atomic (tmp + rename) so concurrent
    // workers don't see half-written files. Same-content races are
    // benign (last writer wins; bytes are deterministic).
    if let Some(cache_p) = fixture_o_cache {
        if let Some(parent) = cache_p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = cache_p.with_extension(format!("tmp-{}-{}.o", std::process::id(), rand_suffix()));
        if std::fs::copy(&obj_path, &tmp).is_ok() {
            let _ = std::fs::rename(&tmp, cache_p);
            write_uses_fetch_sidecar(cache_p, uses_fetch);
        }
    }

    // Hand off to the link stage. obj_path will be cleaned up by the
    // link function (it removes the .o it was given). uses_fetch is
    // passed explicitly so the link path doesn't need ssa_module.
    link_object_to_binary(
        &obj_path,
        out_path,
        opt,
        source_path,
        target,
        kind,
        uses_fetch,
    )
}
