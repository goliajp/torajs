//! Tiny abort helper — panic-free way to bail on invariant
//! violations in torajs staticlibs.
//!
//! Layer-0 substrate (polish A3a, 2026-05-24). Replaces the Rust
//! `expect("msg")` / `panic!("msg")` / `assert!(cond, "msg")`
//! sites in every Layer-1+ staticlib. Those macros expand to
//! calls into `core::panicking::panic` which pulls in:
//!
//! - `std::panicking` — panic handler dispatch
//! - `std::backtrace_rs` — frame capture
//! - `gimli` + `addr2line` — DWARF frame decoders
//! - `rustc_demangle` — symbol demangler
//! - `std::io::Error` + `std::thread::Thread` — paths these touch
//!
//! All told: ~150 KB of dead weight in every user binary that's
//! never executed (the panic path triggers `tr build` developer
//! errors, not user-program errors). Replacing the panic sites
//! with `abort_with(b"msg")` cuts that whole tree.
//!
//! ## Usage
//!
//! Replace:
//!
//! ```rust,ignore
//! let v = some_option.expect("oom");
//! let m = mutex.lock().expect("poisoned");
//! assert!(idx < len, "OOB");
//! ```
//!
//! With:
//!
//! ```rust,ignore
//! use torajs_abort::abort_with;
//! let v = some_option.unwrap_or_else(|| abort_with(b"oom"));
//! let m = mutex.lock().unwrap_or_else(|_| abort_with(b"poisoned"));
//! if idx >= len { abort_with(b"OOB"); }
//! ```

use core::ffi::c_void;

const STDERR_FILENO: i32 = 2;

unsafe extern "C" {
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
    fn abort() -> !;
}

/// Write `msg` + `\n` to stderr (fd 2) via libc `write(2)`, then
/// call libc `abort()`. Never returns.
///
/// `#[cold]` + `#[inline(never)]` — the call site is the unhappy
/// path; keeping it out-of-line lets the optimizer leave a single
/// `bl abort_with` at the failure point and inline the success
/// path tightly.
///
/// # Safety contract
///
/// `msg` must be a valid byte slice (the static-lifetime bound is
/// a hint to call sites; not enforced — `&[u8]` borrow is checked
/// by Rust normally).
#[cold]
#[inline(never)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_abort_with(msg: *const u8, len: usize) -> ! {
    unsafe {
        if len > 0 {
            write(STDERR_FILENO, msg as *const c_void, len);
        }
        write(STDERR_FILENO, b"\n".as_ptr() as *const c_void, 1);
        abort()
    }
}

/// Rust-callable wrapper — the ergonomic call site. Forwards to
/// the no_mangle extern fn so any caller (even outside the
/// staticlib's Rust dep tree) can resolve `__torajs_abort_with`
/// at link time.
#[cold]
#[inline(never)]
pub fn abort_with(msg: &[u8]) -> ! {
    unsafe { __torajs_abort_with(msg.as_ptr(), msg.len()) }
}
