//! torajs-math — 0-libc IEEE-754 `libm` for tr-built user binaries.
//!
//! Self-hosted re-implementations of the 22 `<math.h>` entrypoints
//! the JS spec mandates (`pow / fmod / sin / cos / tan / asin / ...`).
//! Without these, LLVM lowers `Math.*` intrinsics and the f64 `frem`
//! operator to libc symbols, pulling libSystem libm into every user
//! binary; with this crate's staticlib in the link line, the linker
//! resolves them against in-house implementations first.
//!
//! ## Algorithm provenance
//!
//! Algorithms are textbook IEEE-754: same lineage as musl libm /
//! FreeBSD's libm / Sun's 1993 `fdlibm` reference (public-domain or
//! MIT-licensed historically; algorithm itself is public PL+textbook
//! material). **Code is hand-typed here** — per `.claude/rules/torajs-
//! design-principles.md` pillar #2, "algorithms may be learned from
//! others; code must be our own". Each fn module documents the exact
//! formula / coefficients with a one-line reference to the canonical
//! source.
//!
//! ## Why no Cargo deps
//!
//! Pillar #2 again: a `libm` crate from crates.io would be the
//! ergonomic move, but metal-tier substrate is 0-Cargo-deps by
//! mandate. Every fn here is pure `core::f64` hardware ops plus
//! bit-level `u64::from_bits` / `to_bits` shuffles.
//!
//! ## Sister to torajs-mem
//!
//! Same leaf-shim contract: `#[no_mangle] pub extern "C" fn fmod`
//! etc. exposed at the staticlib boundary. Linker prefers staticlib
//! definitions over `libSystem` dyld stubs (order = link-line order).
//! No `#[global_allocator]` interaction, no syscall, no signal.

// Std-on for the same reason as torajs-mem: the staticlib body is
// pure-core but `cargo test` needs a panic handler; std provides one
// for free, and the `#[no_mangle] pub extern "C"` exports stay
// plain-C regardless. Workspace-wide no_std rollout flips this crate
// when the other Layer-0 siblings do.

mod exp;
mod fmod;
mod log;
mod pow;
mod trig;

pub use exp::exp;
pub use fmod::fmod;
pub use log::log;
pub use pow::pow;
pub use trig::{cos, sin, tan};
