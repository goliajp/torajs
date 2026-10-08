//! Can a process execute its own signed file pages mapped a second
//! time at an address of its choosing, and what does that cost next to
//! copying the same bytes into a MAP_JIT region?
//!
//! The binary carries a segment-aligned blob in `__TEXT,__const`, sized
//! like the prelinked runtime's text; every 16 KiB page starts with
//! `mov x0, #page; ret`. Each mode runs in a child process (a code
//! signing violation kills the process) that claims the preferred
//! base, puts the blob there, calls into every page and checks the
//! returned page number:
//! - `file`: mmap the blob's range of the executable PROT_READ|PROT_EXEC
//! - `jit`: MAP_JIT region, copy under pthread_jit_write_protect_np,
//!   invalidate the icache
//! - `anon-rx`: anonymous RW, copy, mprotect RX, invalidate the icache
//! - `copy` / `tampered` (controls): `file` against a byte-identical copy
//!   of the executable, and against one with a byte of the blob changed,
//!   each after registering the copy's signature; the copy should run,
//!   the tampered one should be killed when its changed page is run
//!
//! It also reports which bases a fixed, non-overwriting allocation can
//! claim and where an anonymous mmap hinted at the base lands.
//!
//! Run: `cargo run --release -p torajs-link --example probe_selfmap_exec
//! -- [runs]`.

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
mod probe;

#[cfg(all(target_arch = "aarch64", target_vendor = "apple"))]
fn main() {
    probe::run();
}

#[cfg(not(all(target_arch = "aarch64", target_vendor = "apple")))]
fn main() {
    eprintln!("probe_selfmap_exec runs on arm64 macOS only");
}
