//! Build script — declare `torajs_panic_runtime_active` as a known
//! cfg name so workspace `warnings = "deny"` doesn't trip on
//! `#[cfg(torajs_panic_runtime_active)]` in `src/lib.rs`.
//!
//! Active mode is set by `scripts/release-build.sh` via
//! `RUSTFLAGS="--cfg=torajs_panic_runtime_active"`. Without the flag,
//! the crate compiles as an empty placeholder (see `src/lib.rs`).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(torajs_panic_runtime_active)");
}
