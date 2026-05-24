//! Object-to-binary linking + the runtime `.c` / Layer-1+ `.a`
//! staticlib bake-in pipeline.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition (2026-05-25).
//! `link_object_to_binary` is the post-IR link step that turns an
//! LLVM-emitted `.o` into the final executable / shared lib + bakes
//! the workspace's 24 `libtorajs_*.a` staticlibs + cc-compiles the
//! torajs-runtime crate's remaining `runtime_*.c` sources (only
//! `runtime_libc_bridge.c` post-Phase-1).
//!
//! Also owns:
//! - the wasm32-wasip1 toolchain locator (brew prefix lookup)
//! - per-runtime-`.o` cache (key derivation + dir + sidecar)
//! - the per-`tr build` random-suffix helper

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CompileError, CompileTarget, OutputKind};

pub(super) fn link_object_to_binary(
    obj_path: &Path,
    out_path: &Path,
    opt: &str,
    source_path: Option<&Path>,
    target: CompileTarget,
    kind: OutputKind,
    uses_fetch: bool,
) -> Result<(), CompileError> {
    let _ = opt; // opt is captured by cc_opt_arg derivation below; silence unused
    // M6.1+ — torajs's C runtime. Pieces that are clearer in C than
    // via the inkwell IR-builder API (string split, array join,
    // anything future where IR builder verbosity outweighs the
    // link-cost gain). Embedded via include_str! in torajs-runtime
    // and recompiled fresh per `tr build`; adds ~10-30 ms to the
    // AOT pipeline per C TU (negligible vs LLVM optimize).
    //
    // Each .c declares its own copy of __torajs_heap_header_t (binary
    // compatible) and links against __torajs_rc_dec from runtime_str.c.
    // Each compiles to its own .o; all link with the LLVM-emitted main .o.
    //
    // v0.3 #6 Graduation — C sources live in the torajs-runtime crate
    // so their ABI is locked behind a stable crate boundary. Sources
    // enumerated in `torajs_runtime::SOURCES` so adding a new TU is
    // a single line in lib.rs (no per-file scaffolding here). v0.5
    // T-15 added runtime_promise.c via this path.
    let pid = std::process::id();

    // P2.2+ (architecture-rewrite): every Layer-1+ Rust sub-crate
    // contributing `__torajs_*` symbols ships its staticlib bytes
    // via `crate::TORAJS_STATICLIBS` (assembled at compile time
    // by `crates/torajs-core/build.rs`). We drop each one into a
    // per-build temp `.a` here and collect the paths to append to
    // the link command below.
    let mut rust_staticlib_paths: Vec<PathBuf> = Vec::with_capacity(crate::TORAJS_STATICLIBS.len());
    for (filename, bytes) in crate::TORAJS_STATICLIBS {
        let stem = filename.trim_end_matches(".a");
        let p = std::env::temp_dir().join(format!("{stem}-{pid}-{}.a", rand_suffix()));
        std::fs::write(&p, bytes)
            .map_err(|e| CompileError::Link(format!("write {filename} temp: {e}")))?;
        rust_staticlib_paths.push(p);
    }

    let mut c_paths: Vec<PathBuf> = Vec::with_capacity(torajs_runtime::SOURCES.len());
    let mut o_paths: Vec<PathBuf> = Vec::with_capacity(torajs_runtime::SOURCES.len());
    for (filename, _) in torajs_runtime::SOURCES {
        let stem = filename.trim_end_matches(".c");
        c_paths.push(
            std::env::temp_dir().join(format!("torajs-runtime-{stem}-{pid}-{}.c", rand_suffix())),
        );
        o_paths.push(
            std::env::temp_dir().join(format!("torajs-runtime-{stem}-{pid}-{}.o", rand_suffix())),
        );
    }
    for (idx, (filename, src)) in torajs_runtime::SOURCES.iter().enumerate() {
        std::fs::write(&c_paths[idx], src)
            .map_err(|e| CompileError::Link(format!("write {filename}: {e}")))?;
    }
    // T-20 (v0.6.0) — for wasm32-wasi, use LLVM 22 clang with the
    // wasm32-wasip1 triple + wasi-libc sysroot from Homebrew. cc on
    // macOS is Apple's clang which doesn't have the WebAssembly
    // backend. wasi_paths_for_target() locates the brew-installed
    // toolchain at runtime so the developer doesn't have to set
    // env vars (the prefix lookup is one process spawn at compile
    // time, dominated by LLVM's optimize pass anyway).
    let (cc_cmd, cc_target_args, cc_opt_arg, link_cmd_name): (&str, Vec<String>, &str, &str) =
        match target {
            CompileTarget::Native => ("cc", Vec::new(), "-O3", "cc"),
            CompileTarget::Wasm32Wasi => {
                let (clang_path, sysroot) = wasi_paths_for_target()?;
                (
                    Box::leak(clang_path.into_boxed_str()),
                    vec![
                        "--target=wasm32-wasip1".into(),
                        format!("--sysroot={sysroot}"),
                    ],
                    "-O2", // wasm-ld + LTO + O3 hits a verifier issue in
                    // LLVM 22; O2 is the documented stable level
                    // for the wasm backend (matches Emscripten's
                    // default).
                    "wasm-ld",
                )
            }
        };
    // -flto lets the linker inline cross-TU calls between the
    // LLVM-emitted object and the C runtime.
    //
    // CACHE: each runtime .c file produces a deterministic .o given
    // (source bytes, cc args). 12 files × ~50-100 ms cc invocation
    // each = 0.6-1.2 s wasted per `tr run` before the runtime-obj
    // cache landed. Cache key hashes (source bytes + cc_cmd +
    // cc_target_args + cc_opt_arg + flto/g flags). Hit: copy
    // cached .o → o_paths[idx]. Miss: cc -c, copy to cache, copy
    // to o_paths[idx]. Atomic via temp-then-rename.
    //
    // Cache lives in the same `~/.torajs/cache/` dir as fixture
    // binaries with prefix `runtime-` so the existing LRU prune
    // covers them. Same TORAJS_NO_CACHE / TORAJS_CACHE_DIR env
    // overrides apply.
    let runtime_cache_dir = runtime_cache_dir_for(target, opt);
    for (idx, (filename, src)) in torajs_runtime::SOURCES.iter().enumerate() {
        if let Some(cache_dir) = runtime_cache_dir.as_ref() {
            let key = runtime_obj_cache_key(
                filename,
                src.as_bytes(),
                cc_cmd,
                &cc_target_args,
                cc_opt_arg,
                target,
            );
            let cache_path = cache_dir.join(format!("runtime-{key}.o"));
            if cache_path.is_file() && std::fs::copy(&cache_path, &o_paths[idx]).is_ok() {
                continue;
            }
        }
        let mut cmd = Command::new(cc_cmd);
        cmd.args(["-c"]).arg(cc_opt_arg);
        for ta in &cc_target_args {
            cmd.arg(ta);
        }
        if matches!(target, CompileTarget::Native) {
            cmd.arg("-flto").arg("-g");
        }
        let status = cmd
            .arg("-o")
            .arg(&o_paths[idx])
            .arg(&c_paths[idx])
            .status()
            .map_err(|e| CompileError::Link(format!("spawning cc -c ({filename}): {e}")))?;
        if !status.success() {
            for p in &c_paths {
                let _ = std::fs::remove_file(p);
            }
            for p in o_paths.iter().take(idx) {
                let _ = std::fs::remove_file(p);
            }
            return Err(CompileError::Link(format!(
                "cc -c {filename} exited {status}"
            )));
        }
        // Cache the freshly-produced .o for future runs.
        if let Some(cache_dir) = runtime_cache_dir.as_ref() {
            let key = runtime_obj_cache_key(
                filename,
                src.as_bytes(),
                cc_cmd,
                &cc_target_args,
                cc_opt_arg,
                target,
            );
            let cache_path = cache_dir.join(format!("runtime-{key}.o"));
            let _ = std::fs::create_dir_all(cache_dir);
            // Atomic: write to tmp + rename. Multiple workers racing
            // on the same key will all produce identical bytes; the
            // last rename wins, harmless.
            let tmp = cache_dir.join(format!(
                "runtime-{key}.o.tmp-{}-{}",
                std::process::id(),
                rand_suffix()
            ));
            if std::fs::copy(&o_paths[idx], &tmp).is_ok() {
                let _ = std::fs::rename(&tmp, &cache_path);
            }
        }
    }

    // v0.3 #4 D-2 — `-g` keeps DWARF live through the link stage.
    // On macOS the linker writes a separate `.dSYM` bundle alongside
    // the binary by default; D-4 will pick the right resolver path
    // for `atos` symbolication. Cost is link-time only — runtime
    // perf unaffected.
    //
    // T-20 (v0.6.0) — for wasm32-wasi, link via wasm-ld with the
    // wasi-libc sysroot. The wasi-sdk's libc.a + libwasi-emulated-
    // mman + crt1-command.o provide the wasi syscall ABI; without
    // these wasm-ld can't resolve printf / malloc / fopen / etc.
    let mut link_cmd = Command::new(link_cmd_name);
    match target {
        CompileTarget::Native => {
            link_cmd.arg("-flto").arg("-g").arg(obj_path);
            for op in &o_paths {
                link_cmd.arg(op);
            }
            // P2.2+ — Layer-1+ Rust staticlibs: each supplies its
            // own `__torajs_*` symbols (torajs-rc → rc_inc/dec;
            // torajs-anyvalue → any_box/unbox/drop/payload_rc_inc).
            // Order doesn't matter for cc -flto archive consumption;
            // the linker pulls in whichever members are referenced
            // by `*.o` symbols above.
            for p in &rust_staticlib_paths {
                link_cmd.arg(p);
            }
            /* T-21 (v0.6.0) — runtime_fetch.c uses libcurl for the
             * sync HTTP fetch. Only link libcurl when the user
             * program actually references `fetch(...)`; otherwise
             * dyld would still load libcurl + its TLS deps at
             * process start, regressing every short-running case
             * by ~0.7ms (fifo-queue-100k / stack-pop-1m / startup).
             *
             * Detection: scan the SSA module for any Call whose
             * callee is the fetch_sync intrinsic (declared by
             * ssa_lower only when the program contains a `fetch`
             * call site). Keep this conditional sharp — adding
             * libcurl for a feature the program doesn't use is
             * dead weight. */
            if uses_fetch {
                link_cmd.arg("-lcurl");
            }
            // V3-16 — shared-lib output: cc's `-shared` flag asks
            // ld for a position-independent dylib (no main, no
            // crt1). On macOS this becomes `-dynamiclib` under the
            // hood; cc handles the per-platform translation.
            // `-fPIC` makes every per-TU object position-
            // independent so the loader can map at any address.
            // `-undefined dynamic_lookup` defers symbol resolution
            // for runtime intrinsics (`__torajs_str_alloc`, etc)
            // to the host process — when the dylib is loaded into
            // a tora-emitted binary, the host already has those
            // symbols and the loader binds them.
            if matches!(kind, OutputKind::SharedLib) {
                link_cmd.arg("-shared").arg("-fPIC");
                #[cfg(target_os = "macos")]
                link_cmd.arg("-Wl,-undefined,dynamic_lookup");
            }
            // Polish A2 (2026-05-24) — strip cross-archive dead code.
            // Pre-polish each user binary embedded all 24 libtorajs_*.a
            // via the LTO link, but no_mangle symbols are treated as
            // ABI-export by LTO and not DCE'd even when unreferenced.
            // ld's post-LTO -dead_strip walks the symbol graph from
            // _main (the user binary's entry) and removes anything
            // unreachable. On a fib40-only program this collapses
            // from ~445 KB → ~50 KB (the libtorajs_str / arr / promise
            // / etc. surfaces fib40 never calls).
            //
            // ExecutableBinary only — dylib output needs every symbol
            // available for the host process to bind at load time.
            if matches!(kind, OutputKind::Executable) {
                #[cfg(target_os = "macos")]
                link_cmd.arg("-Wl,-dead_strip");
                #[cfg(target_os = "linux")]
                link_cmd.arg("-Wl,--gc-sections");
            }
            link_cmd.arg("-o").arg(out_path);
        }
        CompileTarget::Wasm32Wasi => {
            let (_clang_path, sysroot) = wasi_paths_for_target()?;
            // wasm-ld doesn't pull libc on its own; pass the wasi-
            // sysroot lib directories explicitly + the crt entry
            // object so `_start` lands at module init.
            link_cmd.arg(format!("{sysroot}/lib/wasm32-wasip1/crt1-command.o"));
            link_cmd.arg(obj_path);
            for op in &o_paths {
                link_cmd.arg(op);
            }
            // P2.2+ — same Layer-1+ staticlibs on wasm. NOTE:
            // each .a is built with the workspace's host target
            // (e.g. aarch64-apple-darwin) and is NOT directly
            // wasm32-wasi-compatible; this leg of the link will
            // currently fail. Wasm-arch cross-build of every Rust
            // sub-crate is queued as a follow-up (L3b).
            for p in &rust_staticlib_paths {
                link_cmd.arg(p);
            }
            link_cmd
                .arg(format!("-L{sysroot}/lib/wasm32-wasip1"))
                .arg("-lc")
                .arg("--no-entry") // crt1-command.o supplies _start
                .arg("--export=_start")
                .arg("-o")
                .arg(out_path);
        }
    }
    let status = link_cmd
        .status()
        .map_err(|e| CompileError::Link(format!("spawning {link_cmd_name}: {e}")))?;
    // v0.3 #4 D-2 — macOS: consolidate DWARF from per-TU .o files
    // into a `.dSYM` bundle alongside the binary. atos / lldb find
    // it automatically by name. Without this, the .o files we're
    // about to delete take their DWARF with them and backtraces
    // can't resolve to source. linux embeds DWARF directly in the
    // binary so no consolidation step is needed.
    #[cfg(target_os = "macos")]
    if source_path.is_some() && matches!(target, CompileTarget::Native) {
        // Silence dsymutil's `warning: (arm64) /tmp/lto.o unable to
        // open object file` — that's the LTO temp .o which the
        // linker has already deleted by the time dsymutil runs;
        // benign but pollutes stderr's first line and breaks
        // test262's classifier (it reads the leading line to
        // decide incompat vs bug).
        let _ = Command::new("dsymutil")
            .arg(out_path)
            .stderr(std::process::Stdio::null())
            .status();
        // Polish A2 — strip in-binary debug info now that the .dSYM
        // bundle owns it. The -g flag at link time embedded DWARF
        // into __LINKEDIT (~140 KB on fib40); after dsymutil the
        // bundle has it for backtrace symbolication and the in-
        // binary copy is dead weight. `strip -S` keeps regular
        // symbols (so backtrace can still print fn names without
        // the .dSYM) but drops debug sections. ExecutableBinary
        // only — dylib output keeps debug so consumers can debug
        // through the dlopen.
        if matches!(kind, OutputKind::Executable) {
            let _ = Command::new("strip")
                .arg("-S")
                .arg(out_path)
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = source_path; // silence unused warning on non-mac
    let _ = std::fs::remove_file(obj_path);
    for p in &c_paths {
        let _ = std::fs::remove_file(p);
    }
    for p in &o_paths {
        let _ = std::fs::remove_file(p);
    }
    // P2.2+ — clean up every embedded Rust staticlib temp file.
    for p in &rust_staticlib_paths {
        let _ = std::fs::remove_file(p);
    }
    if !status.success() {
        return Err(CompileError::Link(format!("cc exited {status}")));
    }
    Ok(())
}

/// T-20 (v0.6.0) — locate the LLVM 22 clang + wasi-libc sysroot
/// installed by Homebrew. Both are required to compile + link
/// wasm32-wasip1 binaries; macOS's system clang doesn't have the
/// WebAssembly backend and there's no canonical wasi sysroot path.
/// `brew --prefix <pkg>` is one process spawn at compile time —
/// dominated by LLVM's optimize pass which runs unconditionally.
pub(super) fn wasi_paths_for_target() -> Result<(String, String), CompileError> {
    fn brew_prefix(pkg: &str) -> Result<String, CompileError> {
        let out = Command::new("brew")
            .args(["--prefix", pkg])
            .output()
            .map_err(|e| {
                CompileError::Link(format!(
                    "wasm32-wasi target needs `brew --prefix {pkg}`: {e} \
                     (install via `brew install {pkg}`)"
                ))
            })?;
        if !out.status.success() {
            return Err(CompileError::Link(format!(
                "brew --prefix {pkg} exited {} — install via `brew install {pkg}`",
                out.status
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
    let llvm_prefix = brew_prefix("llvm@22")?;
    let wasi_prefix = brew_prefix("wasi-libc")?;
    let clang_path = format!("{llvm_prefix}/bin/clang");
    let sysroot = format!("{wasi_prefix}/share/wasi-sysroot");
    Ok((clang_path, sysroot))
}

/// Sub-ns random suffix for per-build temp file names. Cheap +
/// unique-enough for the few hundred parallel `tr build` invocations
/// a single host might run (the bench / conformance harness already
/// collision-tests this empirically).
pub(super) fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
}

/// Per-fixture .o cache sidecar — records `uses_fetch` so the
/// fast (cache-hit) link path knows whether to add `-lcurl` without
/// needing the original SSA module. Sidecar is just an empty file
/// next to the .o, named `<key>.uses_fetch` — presence = true,
/// absence = false. Write is best-effort (cache miss on read just
/// falls back to false, which only matters for fetch-using fixtures).
pub(super) fn write_uses_fetch_sidecar(o_path: &Path, uses_fetch: bool) {
    let sidecar = o_path.with_extension("uses_fetch");
    if uses_fetch {
        let _ = std::fs::write(&sidecar, b"");
    } else {
        let _ = std::fs::remove_file(&sidecar);
    }
}

/// Read the `uses_fetch` sidecar for `o_path`. Missing sidecar =
/// false (most fixtures don't use fetch; -lcurl is only needed when
/// the fixture actually calls into runtime_fetch.c).
pub(super) fn read_uses_fetch_sidecar(o_path: &Path) -> bool {
    o_path.with_extension("uses_fetch").is_file()
}

/// Locate `~/.torajs/cache` (or `$TORAJS_CACHE_DIR`) for the runtime
/// .o cache. Returns `None` when `TORAJS_NO_CACHE` is set (matches
/// the binary cache opt-out) or when neither env var nor `$HOME`
/// resolves. Target / opt-level differ in cache key not directory.
fn runtime_cache_dir_for(_target: CompileTarget, _opt: &str) -> Option<PathBuf> {
    if std::env::var_os("TORAJS_NO_CACHE").is_some() {
        return None;
    }
    if let Some(d) = std::env::var_os("TORAJS_CACHE_DIR") {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".torajs/cache"))
}

/// Cache key for one runtime `.c` file's compiled `.o` output.
/// Includes everything that influences the produced bytes:
/// - source content (every byte; substrate ship = source change)
/// - cc command + target args (host vs wasm differs)
/// - opt flag (`-O3` native, `-O2` wasm)
/// - target enum (native vs wasm — encoded for paranoia, redundant
///   with cc_target_args but cheap)
/// - flto/g flags (native-only, added later in the compile fn)
///
/// Same FxHash-via-DefaultHasher shape as `run_cache_key`: false
/// misses are harmless (recompile), false hits are impossible
/// because all relevant inputs are hashed.
fn runtime_obj_cache_key(
    filename: &str,
    source: &[u8],
    cc_cmd: &str,
    cc_target_args: &[String],
    cc_opt_arg: &str,
    target: CompileTarget,
) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    filename.hash(&mut h);
    source.hash(&mut h);
    cc_cmd.hash(&mut h);
    for a in cc_target_args {
        a.hash(&mut h);
    }
    cc_opt_arg.hash(&mut h);
    // CompileTarget doesn't derive Hash; encode as discriminant.
    match target {
        CompileTarget::Native => 0u8.hash(&mut h),
        CompileTarget::Wasm32Wasi => 1u8.hash(&mut h),
    }
    // Native always passes `-flto -g`; wasm doesn't. Encode that.
    let extra_flags = matches!(target, CompileTarget::Native);
    extra_flags.hash(&mut h);
    // Cache version tag — bump if cc invocation shape changes.
    "runtime-obj-v1".hash(&mut h);
    format!("{:016x}", h.finish())
}
