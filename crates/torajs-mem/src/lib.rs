//! torajs-mem — 0-libc memcpy/memmove/memcmp/memset/bzero/strlen
//! linker-symbol shims for tr-built user binaries.
//!
//! Replaces the libc / `compiler_builtins` fallback path for the
//! LLVM-emitted memory intrinsics. Each Rust crate that calls
//! `core::ptr::copy_*` / `<[u8]>::copy_from_slice` /
//! `core::mem::take` etc. ends up with an LLVM call to
//! `@llvm.memcpy.p0.p0.i64` which the codegen lowers to a call
//! to the libc-shape `memcpy` symbol. Without an in-binary
//! definition the linker resolves the symbol against libSystem,
//! pulling libc into the user binary's import table.
//!
//! By providing `#[no_mangle] pub extern "C" fn memcpy / memmove
//! / memcmp / memset / bzero / strlen` in this crate's staticlib, the
//! linker resolves the undefined ref against our symbol first
//! (staticlib order before dyld libSystem). Net effect: tr-built
//! user binaries drop the libSystem dyld stub entries for these
//! symbols.
//!
//! ## Why a separate crate
//!
//! - **Symbol-resolution clarity**: the `#[no_mangle]`
//!   fns must be in a staticlib that the user binary links AS
//!   ONE archive — splitting them across runtime / mmalloc /
//!   etc. confuses the linker order and risks duplicate-symbol
//!   errors. A single crate-staticlib boundary is the textbook
//!   pattern.
//! - **0 deps**: this crate is pure `core` with no other Cargo
//!   path dep. Sits as a sibling to torajs-io / torajs-fmt /
//!   torajs-syscall in the Layer-0 metal-tier.
//! - **Word-wise kernels**: the copy / fill / compare loops move 16
//!   bytes per step through explicit unaligned word loads and stores
//!   (`word.rs`). A byte loop is not vectorized here — the compiler
//!   cannot fold a loop into the very libcall it is compiling — and
//!   costs three instructions a byte. `#![no_builtins]` keeps the
//!   word loops from being folded into calls to themselves.

#![no_builtins]

// Not yet `#![no_std]` — Step 16-a leaves std on so the test
// binary has a panic_handler. The fns below are
// `#[no_mangle] pub extern "C"` so the staticlib still exposes
// them as plain C ABI symbols regardless of std/no_std mode.
// Step 16-c (workspace-wide no_std rollout) flips this crate
// to no_std + an explicit panic_handler.

mod bzero;
mod memcmp;
mod memcpy;
mod memmove;
mod memset;
mod memset_pattern16;
mod strlen;
mod word;

pub use bzero::bzero;
pub use memcmp::memcmp;
pub use memcpy::memcpy;
pub use memmove::memmove;
pub use memset::memset;
pub use memset_pattern16::memset_pattern16;
pub use strlen::strlen;
