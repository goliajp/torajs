//! Build script — emit the path to every Layer-1+ Rust sub-crate's
//! `lib<name>.a` as a cargo env var so `lib.rs` can `include_bytes!`
//! the staticlib bytes directly into the `tr` binary. At `tr build`
//! time, `torajs-link` writes each `.a` to a temp file and hands the
//! paths to the in-house Mach-O linker.
//!
//! Why this dance: `tr` is a Rust binary; `tr build` runs on user
//! machines that have no Rust toolchain. Each Layer-1+ staticlib
//! has to be baked into `tr` at the build time of `tr` itself.
//!
//! Why env var -> read at compile time (not an OUT_DIR copy): a build
//! script runs without waiting for its package's dependencies to
//! compile, so a copy made here can predate the archive. The archives
//! are read inside this crate's compilation (and torajs-cli's), and
//! that is ordered after each one is written only through a
//! `[dependencies]` edge: cargo does not pipeline a dependency whose
//! crate types include `staticlib` (rustc emits it without a separate
//! `metadata` step), so the dependent starts only once that rustc has
//! finished and cargo has lifted `lib<name>.a` into
//! `target/<profile>/`. A listed archive with no edge is read whenever
//! the scheduler happens to get to it — missing on a clean target,
//! stale on a dirty one — so `main` refuses one.

use std::env;
use std::path::PathBuf;

#[path = "build/compiler_sources.rs"]
mod compiler_sources;
use compiler_sources::COMPILER_SOURCE_FILES;

/// Enumerate every Layer-1+ Rust sub-crate that contributes
/// `__torajs_*` symbols to the final `tr build` user binary. New
/// sub-crates added during the architecture rewrite go in this
/// list, with a matching one-line entry in `staticlibs.rs`'s
/// `runtime_image!` call naming `"TORAJS_<NAME>_STATICLIB_PATH"` and
/// a `[dependencies]` entry in this package's Cargo.toml.
const STATICLIBS: &[&str] = &[
    "torajs_syscall",         // Layer-0: aarch64/x86_64 raw syscall trampoline (v0.7-A1)
    "torajs_io",              // Layer-0: 0-libc buffered stdout writer (v0.7-A3 Step 14-a)
    "torajs_fmt",             // Layer-0: 0-libc f64 ↔ string conversion (v0.7-A4 Step 15-a/b/c)
    "torajs_mem", // Layer-0: 0-libc memcpy/memmove/memcmp linker shims (v0.7-A5 Step 16-a)
    "torajs_mmalloc", // Layer-0: mmap-backed allocator + libc-compat shim (v0.7-A2)
    "torajs_rc",  // Layer-1: refcount + heap-header
    "torajs_anyvalue", // Layer-1: AnyBox (boxed Type::Any)
    "torajs_dispatch", // Layer-1: any-method dispatch link seam — thin forwarder member a specialized user-.o dispatcher shadows (RFC 20260824-s2-5 blade 0)
    "torajs_throw",    // Layer-1: native-error registry + throw helpers
    "torajs_str",      // Layer-2: Str layout + small-Str pool + alloc/free
    "torajs_num",      // Layer-2: Number primitives + Math namespace intrinsics
    "torajs_math", // Layer-0: 0-libc IEEE-754 libm (fmod / pow / sin / cos / ...) — drops libSystem libm import (v0.7-A5 Step ⑦)
    "torajs_bigint", // Layer-2: BigInt arbitrary-precision integer (P3.3)
    "torajs_arr",  // Layer-3: Array<T> + Array<Any> substrate (P4.1)
    "torajs_dynobj", // Layer-3: dynamic-property object hashmap (P4.2)
    "torajs_collections", // Layer-3: Map<K,V> + Set + MapIter (P4.3)
    "torajs_weak", // Layer-3: WeakRef + WeakMap + WeakSet substrate (P4.3')
    "torajs_cycle", // Layer-3: Bacon-Rajan trial-deletion cycle collector (P4.4)
    "torajs_microtask", // Layer-3: microtask queue (P5)
    "torajs_promise", // Layer-3: Promise surface — alloc/pool/drop/state/then/combinator/queueMicrotask (P6.1)
    "torajs_regex", // Layer-3: ECMAScript regex — parser / Thompson NFA / Pike VM + extern API (P6.2)
    "torajs_fetch", // Layer-3: sync HTTP fetch (libcurl-easy wrapper) — Response heap + drop (P6.3)
    "torajs_date", // Layer-3: Date class — heap layout + ctors / getters / setters / ISO format (P6.4)
    "torajs_capture_box", // Layer-1: refcounted 16B capture box for escape-captured let slots (P6.5)
    "torajs_fs", // Layer-3: synchronous filesystem substrate (readFileSync / writeFileSync / mkdirSync / ...) (P7.d)
    "torajs_meta", // Layer-3: runtime metadata + reflection — fnprops side table + class/proto registries + getPropertyDescriptor / getPrototypeOf (P7.g)
    "torajs_structmeta", // Layer-3: struct-layout reflection read-side over __torajs_class_layouts — layout_lookup / field_name / field_info / field_find (W-J Phase A4)
    "torajs_process", // Layer-3: process surface — exit / cwd / env / argv / platform / stdout.write / stderr.write (P7.h-proc)
    "torajs_panic", // Layer-1: central fatal-error helper — stderr msg + symbolicated backtrace + exit (P7.i-panic)
    "torajs_panic_runtime", // Layer-0: custom #![panic_runtime] + #[panic_handler] replacing std default — strips ~150 KiB backtrace/demangle/path/io/Thread tree from user binaries (v0.7 Step 9b)
    "torajs_value_drop", // Layer-1: universal heap-typed drop dispatch — type_tag → per-type _drop (P7.i-drop)
    "torajs_buffer", // Layer-3: ArrayBuffer / TypedArray / DataView substrate — RFC 20260823-typedarray-substrate
    "torajs_wrapper", // Layer-2: primitive-wrapper heap objects (NumberWrapper / StringWrapper / BooleanWrapper) — RFC 20260716-primitive-wrapper-substrate 刀 1
    "torajs_abort", // Layer-0: panic-free abort helper — write(2)+abort(); replaces Rust panic infra (polish A3a)
    "torajs_print", // Layer-0: console primitives (print_bool / print_i64 / print_f64) — co-linked by torajs-link at `tr build`.
    "torajs_fnname", // Layer-1: fn-name registry runtime helper — looks up fn body vaddr in __torajs_fn_name_table[] and emits [Function: <name>] / [Function (anonymous)] for inspect (fn-name registry Phase 2 Step 4).
    "torajs_anyvalue_own", // Layer-1: the own walk of the any -> class-slot object cast behind its two entries; near last, so every member it shares resolves to the archive that owns it
    "torajs_regexp_lane", // Layer-1: the RegExp protocol seam's body (kernel lane or algorithm); near last, so every member it shares resolves to the archive that owns it
    "torajs_regexp_protocol", // Layer-1: RegExp protocol algorithms + ctor build behind two seams; last, so every member it shares resolves to the archive that owns it
];

fn main() {
    // Opt-in flag the census build sets (RUSTFLAGS=--cfg=torajs_census);
    // this crate has no [lints] table, so it declares the name itself.
    println!("cargo::rustc-check-cfg=cfg(torajs_census)");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));
    require_dependency_edges();

    // OUT_DIR layout (cargo internal):
    //   <target>/<profile>/build/<crate>-<hash>/out
    // Pop 3 to reach <target>/<profile>/ where cargo emits
    // library artifacts (lib*.rlib, lib*.a, ...).
    let mut target_profile_dir = out_dir.clone();
    for _ in 0..3 {
        target_profile_dir.pop();
    }

    for name in STATICLIBS {
        let filename = format!("lib{name}.a");
        let src = target_profile_dir.join(&filename);
        // Emit the absolute path as a compile-time env var so
        // lib.rs's `include_bytes!(env!("TORAJS_<NAME>_STATICLIB
        // _PATH"))` resolves to the correct file. Uppercase the
        // sub-crate name to match Cargo's env-var conventions.
        let env_name = format!("{}_STATICLIB_PATH", name.to_uppercase());
        println!("cargo:rustc-env={}={}", env_name, src.display());
        println!("cargo::metadata={}={}", env_name, src.display());
        // `cargo:rerun-if-env-changed` ensures rustc re-runs the
        // crate that consumes this env var whenever cargo's env
        // for it changes. Combined with the always-rerun sentinel
        // below, this keeps the embed in sync across edits.
        println!("cargo:rerun-if-env-changed={}", env_name);
    }

    // NO always-rerun sentinel here, deliberately.
    //
    // This used to be `cargo:rerun-if-changed=NULL_FORCE_RERUN` — the
    // documented "always rerun" idiom — to make a sub-crate's staticlib
    // edit reach this crate. The note beside it said an unconditional
    // rerun was "essentially free" because the script body is a handful
    // of printlns. That accounted for the script and not for what cargo
    // does with it: a dirty build script makes every downstream unit
    // dirty too, so each no-op `cargo build --workspace --release`
    // rebuilt torajs-core, -egraph, -codegen, -obj, -link and `tr` —
    // 72 seconds to compile nothing at all, in front of every gate,
    // sweep and scan (measured 2026-09-07, cargo's own
    // `fingerprint=info` naming MissingFile{NULL_FORCE_RERUN} as the
    // root and StaleDepFingerprint for all fivedownstream units).
    //
    // What it was protecting is already covered: `include_bytes!` reads
    // each `lib<name>.a` at THIS crate's compile time, so rustc records
    // all 36 of them in `torajs_core-<hash>.d` and cargo re-runs the
    // crate when one changes. That is the mechanism rotation 585
    // confirmed when it corrected the opposite belief — one
    // `cargo build --workspace` picks a runtime-crate edit up without
    // any sentinel. `hardev/autorun/staticlib_freshness.sh` is the
    // mechanical check that this stays true.

    // Compute a compiler-source fingerprint for the per-fixture .o
    // cache key (B-1 phase 2). Hash every `.rs` file that affects
    // codegen output; emit as `TORAJS_COMPILER_REV` so the cli can
    // put it in the cache key. Substrate ships don't change these
    // files → cache stays warm across substrate ships, even though
    // `tr` binary mtime does change. Compiler-logic ships (touching
    // ssa_lower / check / etc.) flip the hash and invalidate the
    // cache — correct semantics.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for rel in COMPILER_SOURCE_FILES {
        // Each file's path AND bytes go in. Path catches reorders;
        // bytes catch content edits.
        rel.hash(&mut h);
        match std::fs::read(rel) {
            Ok(bytes) => bytes.hash(&mut h),
            Err(e) => {
                // Fail the build — silently downgrading to "no compiler
                // hash" would let cache silently serve stale entries
                // across compiler-logic ships.
                panic!("build.rs: cannot read compiler source `{rel}`: {e}");
            }
        }
        // Tell cargo to rerun build.rs when any compiler source changes.
        // Without this, cargo's fingerprint logic might skip rebuilds.
        println!("cargo:rerun-if-changed={rel}");
    }
    println!("cargo:rustc-env=TORAJS_COMPILER_REV={:016x}", h.finish());
}

/// Every archive in `STATICLIBS` must be a `[dependencies]` entry of
/// this package: the edge is what orders its compile before the read.
fn require_dependency_edges() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest = std::fs::read_to_string("Cargo.toml")
        .expect("build.rs: cannot read torajs-core Cargo.toml");
    let deps = manifest
        .split_once("\n[dependencies]\n")
        .map(|(_, rest)| rest.split("\n[").next().unwrap_or(rest))
        .expect("build.rs: torajs-core Cargo.toml has no [dependencies] table");
    let missing: Vec<String> = STATICLIBS
        .iter()
        .map(|name| name.replace('_', "-"))
        .filter(|pkg| !deps.lines().any(|l| l.starts_with(&format!("{pkg} = "))))
        .collect();
    assert!(
        missing.is_empty(),
        "build.rs: STATICLIBS lists {missing:?} without a [dependencies] edge, \
         so nothing orders their archives before this crate reads them"
    );
}
