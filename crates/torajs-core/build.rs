//! Build script — copy every Layer-1+ Rust sub-crate's
//! `lib<name>.a` into `OUT_DIR` so `lib.rs` can `include_bytes!`
//! them and ship the staticlib bytes inside the `tr` binary. At
//! `tr build` time, `ssa_inkwell::compile()` writes each `.a` to
//! a temp file and passes the resulting paths to the link command
//! alongside the runtime_*.c object files.
//!
//! Why this dance: `tr` is a Rust binary; `tr build` runs on user
//! machines that have no Rust toolchain. Each Layer-1+ staticlib
//! has to be baked into `tr` at the build time of `tr` itself.
//! `cargo build` emits `lib<name>.a` at `target/<profile>/lib<name>.a`
//! when the sub-crate's `Cargo.toml` declares
//! `crate-type = ["rlib", "staticlib"]`; this script picks each
//! one up and copies it into `OUT_DIR`, where `include_bytes!`
//! resolves it via the `OUT_DIR` env macro.
//!
//! Cargo guarantees dependency build order — `torajs-core` depends
//! on every sub-crate listed below, so by the time this build
//! script runs each staticlib already exists at the expected path.

use std::env;
use std::path::PathBuf;

/// Enumerate every Layer-1+ Rust sub-crate that contributes
/// `__torajs_*` symbols to the final `tr build` user binary. Each
/// entry is the lib<basename>.a → out-file name mapping. New
/// sub-crates added during the architecture rewrite go in this
/// list, NOT in a parallel hand-copy.
const STATICLIBS: &[&str] = &[
    "torajs_rc",       // Layer-1: refcount + heap-header
    "torajs_anyvalue", // Layer-1: AnyBox (boxed Type::Any)
];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for name in STATICLIBS {
        // Rerun if the sub-crate's lib.rs or Cargo.toml moves —
        // the dep graph already triggers a rebuild of the .a, but
        // we also need build.rs itself re-run so the copy step
        // refreshes the embedded bytes.
        let crate_dir = name.replace('_', "-");
        println!("cargo:rerun-if-changed=../{crate_dir}/src/lib.rs");
        println!("cargo:rerun-if-changed=../{crate_dir}/Cargo.toml");
    }

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
        if !src.exists() {
            panic!(
                "expected {} at {}; check that crates/{}/Cargo.toml has \
                 [lib] crate-type = [\"rlib\", \"staticlib\"] and that \
                 torajs-core depends on it so cargo builds it first",
                filename,
                src.display(),
                name.replace('_', "-"),
            );
        }
        let dest = out_dir.join(&filename);
        std::fs::copy(&src, &dest).unwrap_or_else(|e| {
            panic!("copy {src:?} -> {dest:?}: {e}");
        });
    }
}
