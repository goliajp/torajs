//! `process.*` surface for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate (P7.h-proc, 2026-05-24) — replaces the
//! `process_*` family in `runtime_str.c`. Covers:
//!
//! - `process.exit(code)` — `libc::exit` (no return)
//! - `process.cwd() → string` — `libc::getcwd`
//! - `process.env.NAME → string | undefined` — `libc::getenv`
//! - `process.argv → string[]` — populated by `__torajs_argv_init`
//!   at LLVM-emitted `main` entry
//! - `process.platform → string` — static `"darwin"` / `"linux"` / etc.
//! - `process.stdout.write(str)` — libc `printf` (shared stdio buffer)
//! - `process.stderr.write(str)` — libc `write(2)` direct to fd 2
//!
//! All 8 fns are thin libc wrappers (per-fn body ≤ 20 LOC) — kept
//! in one file under the "thin-interface fn group" exception in
//! `.claude/rules/common/file-size.md` (otherwise the "one file =
//! one fn doing one thing" polish rule would force a per-fn split).
//!
//! ## Buffering
//!
//! `stdout.write` goes through `printf` to share the C stdio stdout
//! buffer with `print_i64` / `print_bool` / `__torajs_str_print` (so
//! mixed write sequences don't reorder). `stderr.write` goes
//! directly to fd 2 via `write(2)` — stderr is conventionally
//! unbuffered (line-buffered for TTYs at most), so kernel ordering
//! is preserved without an explicit `fflush`.

use core::ffi::{c_char, c_void};
use std::sync::Mutex;

const STR_HDR_SIZE: usize = 16;
const STR_LEN_OFF: usize = 8;
const STDERR_FILENO: i32 = 2;

unsafe extern "C" {
    fn exit(code: i32) -> !;
    fn getcwd(buf: *mut c_char, size: usize) -> *mut c_char;
    fn getenv(name: *const c_char) -> *const c_char;
    fn strlen(s: *const c_char) -> usize;
    fn printf(fmt: *const u8, ...) -> i32;
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
    // fflush(NULL) flushes every open output stream — used before
    // stderr.write to drain the libc stdio stdout buffer so direct
    // write(2)-to-fd-2 doesn't reorder ahead of buffered console.log
    // output when the user redirects 2>&1.
    fn fflush(stream: *mut c_void) -> i32;
}

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_panic(msg: *const u8) -> !;
}

#[cfg(test)]
unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-process test stub: __torajs_str_alloc_pooled");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_arr_alloc(_cap: u64) -> *mut u8 {
    panic!("torajs-process test stub: __torajs_arr_alloc");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_arr_push(_arr: *mut u8, _val: i64) -> *mut u8 {
    panic!("torajs-process test stub: __torajs_arr_push");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_panic(_msg: *const u8) -> ! {
    panic!("torajs-process test stub: __torajs_panic");
}

#[inline]
unsafe fn alloc_str(payload: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(payload.len() as u64) };
    if !payload.is_empty() {
        unsafe {
            core::ptr::copy_nonoverlapping(payload.as_ptr(), s.add(STR_HDR_SIZE), payload.len())
        };
    }
    s
}

#[inline]
unsafe fn alloc_str_from_cstr(c: *const c_char) -> *mut u8 {
    let len = unsafe { strlen(c) };
    let s = unsafe { __torajs_str_alloc_pooled(len as u64) };
    if len > 0 {
        unsafe { core::ptr::copy_nonoverlapping(c as *const u8, s.add(STR_HDR_SIZE), len) };
    }
    s
}

#[inline]
unsafe fn str_len(s: *const u8) -> u64 {
    unsafe { (s.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_data(s: *const u8) -> *const u8 {
    unsafe { s.add(STR_HDR_SIZE) }
}

/// `process.exit(code)` — libc exit. Does not return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_exit(code: i64) -> ! {
    unsafe { exit(code as i32) }
}

/// `process.cwd()` → fresh Str. Empty Str on getcwd failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_cwd() -> *mut u8 {
    let mut buf = [0i8; 4096];
    let r = unsafe { getcwd(buf.as_mut_ptr(), buf.len()) };
    if r.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    unsafe { alloc_str_from_cstr(buf.as_ptr()) }
}

/// `process.env.NAME` — owned Str or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_getenv(name_str: *const u8) -> *mut u8 {
    let nlen_full = unsafe { str_len(name_str) } as usize;
    let nlen = nlen_full.min(255);
    let mut buf = [0i8; 256];
    unsafe {
        core::ptr::copy_nonoverlapping(str_data(name_str), buf.as_mut_ptr() as *mut u8, nlen);
        buf[nlen] = 0;
    }
    let v: *const c_char = unsafe { getenv(buf.as_ptr()) };
    if v.is_null() {
        return core::ptr::null_mut();
    }
    unsafe { alloc_str_from_cstr(v) }
}

/// `process.platform` → static-cfg string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_platform() -> *mut u8 {
    #[cfg(target_os = "macos")]
    let p: &[u8] = b"darwin";
    #[cfg(target_os = "linux")]
    let p: &[u8] = b"linux";
    #[cfg(target_os = "windows")]
    let p: &[u8] = b"win32";
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let p: &[u8] = b"unknown";
    unsafe { alloc_str(p) }
}

/// Stored `(argc, argv)` captured at LLVM-emitted `main` entry.
static ARGV_STATE: Mutex<(i32, usize)> = Mutex::new((0, 0));

/// `__torajs_argv_init(argc, argv)` — main-entry plumbing.
///
/// # Safety
/// `argv` must outlive the process (kernel-supplied stack frame).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_argv_init(argc: i32, argv: *mut *mut c_char) {
    let mut state = ARGV_STATE
        .lock()
        .expect("torajs-process argv mutex poisoned");
    *state = (argc, argv as usize);
}

/// `process.argv` → fresh Array<Str>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_argv() -> *mut u8 {
    let (argc, argv_addr) = {
        let state = ARGV_STATE
            .lock()
            .expect("torajs-process argv mutex poisoned");
        (state.0, state.1)
    };
    let argv = argv_addr as *mut *mut c_char;
    let mut out = unsafe { __torajs_arr_alloc(argc as u64) };
    for i in 0..argc {
        let cstr = unsafe { argv.add(i as usize).read() };
        let str_v = unsafe { alloc_str_from_cstr(cstr) };
        out = unsafe { __torajs_arr_push(out, str_v as i64) };
    }
    out
}

/// `process.stdout.write(s)` → bool. Via libc printf (shared stdio
/// buffer). fflush(stdout) via fflush(NULL) so the user sees the
/// write before the next syscall (matches pre-port C behavior).
/// Panics on short write (typed-throw deferred).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_stdout_write(s: *const u8) -> bool {
    let dlen = unsafe { str_len(s) } as i32;
    let d = unsafe { str_data(s) };
    let written = unsafe { printf(b"%.*s\0".as_ptr(), dlen, d) };
    if written < 0 || written != dlen {
        unsafe {
            __torajs_panic(b"not yet supported: process.stdout.write short write\0".as_ptr())
        };
    }
    unsafe { fflush(core::ptr::null_mut()) };
    true
}

/// `process.stderr.write(s)` → bool. Via libc write(2) (direct to
/// fd 2). fflush(NULL) before the write drains the stdio stdout
/// buffer so combined `2>&1` redirection preserves caller-order
/// interleaving with `console.log`. Panics on short write /
/// EBADF / EPIPE.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_stderr_write(s: *const u8) -> bool {
    // Drain buffered stdout writes first — write(2)-direct skips
    // libc stdio so without this flush a redirected `2>&1` would
    // see stderr lines printed before still-buffered stdout lines.
    unsafe { fflush(core::ptr::null_mut()) };
    let dlen = unsafe { str_len(s) } as usize;
    if dlen == 0 {
        return true;
    }
    let d = unsafe { str_data(s) };
    let mut p = d as *const c_void;
    let mut n = dlen;
    while n > 0 {
        let written = unsafe { write(STDERR_FILENO, p, n) };
        if written <= 0 {
            unsafe {
                __torajs_panic(b"not yet supported: process.stderr.write short write\0".as_ptr())
            };
        }
        let w = written as usize;
        p = unsafe { (p as *const u8).add(w) as *const c_void };
        n -= w;
    }
    true
}
