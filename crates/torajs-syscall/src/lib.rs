//! v0.7 Metal — raw syscall stubs for torajs user binary.
//!
//! **Why this exists**: vision #4 "0 deps" in its complete form
//! requires the AOT-emitted user binary to invoke OS services via
//! direct syscalls — no `libc` / `libSystem` dependency, no
//! `extern { fn malloc/free/write/... }`. This crate provides the
//! Layer-0 substrate every higher Rust sub-crate (mmalloc / io /
//! fmt / str / arr / ...) builds on.
//!
//! ## Scope (v0.7-A1)
//!
//! - aarch64 macOS first (Apple's actual Mach kernel syscall ABI;
//!   intentionally walking past the "Apple doesn't guarantee
//!   syscall stability" disclaimer per takagi's "metal-level
//!   exploration" framing).
//! - aarch64 Linux + x86_64 macOS + x86_64 Linux land in follow-up
//!   sub-steps once the macOS-aarch64 path is fully validated.
//!
//! ## Architecture
//!
//! Two layers:
//! 1. **Trampoline** ([`syscall6`] + friends) — pure inline asm
//!    that loads syscall number into the platform's "syscall reg"
//!    (`x16` on aarch64 macOS) and `svc` / `syscall` instruction;
//!    returns the raw OS-level result.
//! 2. **Safe wrappers** ([`write`] / [`read`] / [`exit`] / [`mmap`]
//!    / [`munmap`]) — typed front-ends that hide the raw register
//!    convention and surface `Result<T, Errno>`.
//!
//! ## Non-goals
//!
//! - Not a general-purpose `nix`-style binding library. Only the
//!   syscalls torajs's runtime sub-crates actually use are exposed.
//! - Not multi-arch on first ship. aarch64 macOS only at v0.7-A1.
//!   See `STATUS.md` for the per-arch matrix.
//!
//! `#![no_std]` is deferred to v0.7-A1 step 4 once the safe-wrapper
//! layer (including an exit-via-syscall panic_handler) is in place.
//! Step 1 scaffold builds under std to keep the cargo dep tree
//! simple until then.

pub mod sysno;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub mod arch_aarch64_macos;

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub use arch_aarch64_macos::{syscall0, syscall1, syscall3, syscall6};

pub mod safe;
pub use safe::{Errno, exit, getpid, mmap_anon_rw, munmap, read, write};
