//! Build script — emit the path to every Layer-1+ Rust sub-crate's
//! `lib<name>.a` as a cargo env var so `lib.rs` can `include_bytes!`
//! the staticlib bytes directly into the `tr` binary. At `tr build`
//! time, `ssa_inkwell::compile()` writes each `.a` to a temp file
//! and passes the resulting paths to the link command alongside
//! the runtime_*.c object files.
//!
//! Why this dance: `tr` is a Rust binary; `tr build` runs on user
//! machines that have no Rust toolchain. Each Layer-1+ staticlib
//! has to be baked into `tr` at the build time of `tr` itself.
//!
//! Why env var → include_bytes (not OUT_DIR copy): an earlier
//! version of this script copied each sub-crate's `.a` into
//! `OUT_DIR` and `lib.rs` did `include_bytes!(env!("OUT_DIR")/...)`.
//! That introduces a race — build.rs can run BEFORE the sub-crate's
//! staticlib emit completes, copying a stale `.a`. The `staticlib`
//! crate-type artifact is emitted in parallel with the rlib, and
//! cargo's dep graph only guarantees the rlib finishes before
//! `torajs-core`'s build.rs runs — not the staticlib. By contrast,
//! `include_bytes!` resolves at THIS crate's compile time (after
//! build.rs, after every sub-crate is fully built), so reading
//! straight from `target/<profile>/lib<name>.a` is always fresh.
//! The env var just teaches rustc where to look — the bytes
//! themselves are read at compile time.

use std::env;
use std::path::PathBuf;

/// Enumerate every Layer-1+ Rust sub-crate that contributes
/// `__torajs_*` symbols to the final `tr build` user binary. New
/// sub-crates added during the architecture rewrite go in this
/// list, with a matching one-line entry in `lib.rs`'s
/// `TORAJS_STATICLIBS` array using `env!("TORAJS_<NAME>_STATICLIB
/// _PATH")`.
const STATICLIBS: &[&str] = &[
    "torajs_rc",       // Layer-1: refcount + heap-header
    "torajs_anyvalue", // Layer-1: AnyBox (boxed Type::Any)
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));

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
        // `cargo:rerun-if-env-changed` ensures rustc re-runs the
        // crate that consumes this env var whenever cargo's env
        // for it changes. Combined with the always-rerun sentinel
        // below, this keeps the embed in sync across edits.
        println!("cargo:rerun-if-env-changed={}", env_name);
    }

    // Force this build script to rerun every cargo invocation —
    // sub-crate source edits invalidate their own rlib/staticlib
    // but cargo otherwise considers torajs-core's build script
    // fingerprint stable. A non-existent sentinel path is cargo's
    // documented "always rerun" idiom (cargo treats missing files
    // as "changed" on every poll). The script's body is just a
    // handful of printlns, so unconditional rerun is essentially
    // free.
    println!("cargo:rerun-if-changed=NULL_FORCE_RERUN");
}
