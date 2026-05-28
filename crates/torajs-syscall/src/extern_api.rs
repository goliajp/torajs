//! C-ABI exports for the libc-free Layer-0 staticlibs.
//!
//! `torajs-panic-runtime` and `torajs-abort` are 0-Cargo-deps crates:
//! they cannot `use torajs_syscall::...` directly without taking a
//! Cargo dependency. Instead they declare these `#[no_mangle]`
//! symbols in their own `unsafe extern "C"` blocks and bind them at
//! final link — the same link-level pattern v0.7 Step 16-d used to
//! route `#[global_allocator]` through mmalloc's `__torajs_libc_malloc`.
//! `torajs-syscall` is `STATICLIBS[0]` (see `torajs-core/build.rs`), so
//! these symbols are present in every `tr build` user binary.
//!
//! Symbols are `__torajs_syscall_*`-namespaced and NEVER named after
//! their libc counterparts (`write` / `abort`). Shadowing a libc
//! symbol name with a `#[no_mangle]` definition tripped Rust std +
//! LLVM hidden assumptions in v0.7 Step 16-a-2 and had to be reverted
//! (see the `feedback_libc_shim_caveat` lesson).

use crate::arch_aarch64_macos::{syscall0, syscall1, syscall3};
use crate::sysno::{SIGABRT, SIGKILL, SYS_EXIT, SYS_GETPID, SYS_KILL, SYS_WRITE};

/// Raw `write(2)` for the libc-free staticlibs. Wraps
/// `syscall3(SYS_WRITE, ...)` and returns the raw kernel result —
/// bytes written on success, or `-errno` (the trampoline re-encodes
/// XNU's carry-set error as a negative return). The `isize` shape
/// mirrors libc `write`'s `ssize_t` so the panic / abort call sites
/// keep their existing `-> isize` extern signature.
///
/// Best-effort: a short or failed write is not retried. The only
/// callers are the panic banner (`torajs-panic-runtime`) and the
/// abort banner (`torajs-abort`), both of which already ignore the
/// return value (a best-effort diagnostic written just before the
/// process terminates).
///
/// # Safety
/// `buf` must point to at least `n` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize {
    unsafe { syscall3(SYS_WRITE, fd as i64, buf as i64, n as i64) as isize }
}

/// `abort(3)` for the libc-free staticlibs. Delivers a real SIGABRT
/// to the current process — the orthodox shape (musl / glibc / the Go
/// runtime / Rust `std::process::abort` all raise SIGABRT rather than
/// synthesize an exit code), which preserves the core-dump / debugger
/// / `WIFSIGNALED` semantics that a bare `exit(134)` would discard.
///
/// If SIGABRT is caught, blocked, or ignored and control returns, the
/// call escalates to the uncatchable `SIGKILL`, then `exit(134)`
/// (= 128 + SIGABRT) plus a spin loop as a final guarantee that the
/// `-> !` contract holds. torajs installs no SIGABRT handler today, so
/// the first `kill` terminates in practice; the fallbacks exist purely
/// for soundness.
///
/// Deliberate simplification vs glibc/musl's reset-disposition-and-
/// re-raise: in the (currently impossible) handler-caught case the
/// process dies by SIGKILL (wait status 137) instead of SIGABRT (134).
/// Faithful reset would need `SYS_SIGACTION`, which is not yet in
/// `sysno`; revisit if torajs ever installs a SIGABRT handler.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_syscall_abort() -> ! {
    unsafe {
        let pid = syscall0(SYS_GETPID);
        syscall3(SYS_KILL, pid, SIGABRT as i64, 0);
        syscall3(SYS_KILL, pid, SIGKILL as i64, 0);
        syscall1(SYS_EXIT, 134);
    }
    loop {
        core::hint::spin_loop();
    }
}
