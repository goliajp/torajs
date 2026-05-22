//! Build script — copy `libtorajs_rc.a` into `OUT_DIR` so `lib.rs`
//! can `include_bytes!` it and ship the staticlib's bytes inside
//! the `tr` binary. At `tr build` time, `ssa_inkwell::compile()`
//! writes those bytes to a temp file and passes the resulting
//! path to the link command, alongside the runtime_*.c object
//! files compiled per-build.
//!
//! Why this dance: `tr` is a Rust binary; `tr build` runs on user
//! machines that have no Rust toolchain. The staticlib has to be
//! baked into `tr` at the build time of `tr` itself. `cargo build`
//! emits `libtorajs_rc.a` at `target/<profile>/libtorajs_rc.a`
//! when `torajs-rc`'s `Cargo.toml` declares
//! `crate-type = ["rlib", "staticlib"]`; this script picks it up
//! and copies it into the build script's `OUT_DIR`, where
//! `include_bytes!` can resolve it via the `OUT_DIR` env macro.
//!
//! Cargo guarantees dependency build order — `torajs-core` depends
//! on `torajs-rc`, so by the time this build script runs the
//! staticlib already exists at the expected path.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../torajs-rc/src/lib.rs");
    println!("cargo:rerun-if-changed=../torajs-rc/Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));

    // OUT_DIR layout (cargo internal):
    //   <target>/<profile>/build/<crate>-<hash>/out
    // Pop 3 to reach <target>/<profile>/ where cargo emits library
    // artifacts (lib*.rlib, lib*.a, lib*.dylib, ...).
    let mut target_profile_dir = out_dir.clone();
    for _ in 0..3 {
        target_profile_dir.pop();
    }

    let staticlib = target_profile_dir.join("libtorajs_rc.a");
    if !staticlib.exists() {
        panic!(
            "expected libtorajs_rc.a at {}; check that torajs-rc's \
             Cargo.toml has [lib] crate-type = [\"rlib\", \"staticlib\"] \
             and that torajs-core depends on torajs-rc so cargo builds \
             it first",
            staticlib.display()
        );
    }

    let dest = out_dir.join("libtorajs_rc.a");
    std::fs::copy(&staticlib, &dest).unwrap_or_else(|e| {
        panic!("copy {staticlib:?} -> {dest:?}: {e}");
    });
}
