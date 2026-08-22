//! `process.*` surface for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate (P7.h-proc, 2026-05-24) — replaces the
//! `process_*` family in `runtime_str.c`. Covers:
//!
//! - `process.exit(code)` — `libc::exit` (no return)
//! - `process.cwd() → string` — `fcntl(open("."), F_GETPATH, buf)`
//!   via torajs-syscall (XNU has no `getcwd` syscall — the orthodox
//!   libc impl is exactly this open/fcntl/close trio)
//! - `process.env.NAME → string | undefined` — manual scan of the
//!   `envp` pointer the LLVM-emitted `main` receives as its 3rd
//!   param (avoids the `_NSGetEnviron` / `getenv` libc dep)
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
use torajs_mutex::Mutex;
use torajs_syscall::sysno::O_RDONLY;
use torajs_syscall::{close, fcntl_getpath, open};

const STR_HDR_SIZE: usize = 16;
const STR_LEN_OFF: usize = 8;
const STDERR_FILENO: i32 = 2;
const PATH_MAX_LEN: usize = 4096;

unsafe extern "C" {
    fn exit(code: i32) -> !;
    fn strlen(s: *const c_char) -> usize;
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
    // v0.7-A3 Step 14-b — 0-libc buffered stdout writer. Replaces
    // libc `printf` + `fflush(NULL)` on the process.stdout.write
    // path. __torajs_io_flush drains torajs-io's process-global
    // line buffer before write(2)-direct stderr so combined
    // `2>&1` redirection still preserves caller-order interleaving.
    fn __torajs_io_write_stdout(buf: *const u8, len: u64);
    fn __torajs_io_flush();
}

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut u8;
    fn __torajs_arr_push(arr: *mut u8, val: i64) -> *mut u8;
    fn __torajs_panic(msg: *const u8) -> !;
    // RFC 20260707 chunk 2 fix-up — the immortal `undefined`
    // sentinel Str cell (torajs-str undef_sentinel.rs); a missing
    // env var IS JS undefined, not null.
    fn __torajs_str_undef() -> *mut u8;
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

#[cfg(test)]
unsafe extern "C" fn __torajs_str_undef() -> *mut u8 {
    panic!("torajs-process test stub: __torajs_str_undef");
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
    unsafe { (s.add(STR_LEN_OFF) as *const u32).read() as u64 }
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

/// `process.cwd()` → fresh Str. Empty Str on failure. Walks the
/// orthodox `open(".") + fcntl(F_GETPATH) + close` recipe, identical
/// to the libc `getcwd(3)` impl on darwin — XNU has no `getcwd`
/// syscall, so this is the path libc itself walks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_cwd() -> *mut u8 {
    let mut buf = [0u8; PATH_MAX_LEN];
    let fd = match unsafe { open(b".\0".as_ptr(), O_RDONLY) } {
        Ok(fd) => fd,
        Err(_) => return unsafe { __torajs_str_alloc_pooled(0) },
    };
    if unsafe { fcntl_getpath(fd, &mut buf) }.is_err() {
        let _ = close(fd);
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let _ = close(fd);
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    unsafe { alloc_str(&buf[..nul]) }
}

/// Scan the captured `envp` block for `name` — `(value ptr, value
/// len)` BORROWED from the kernel-supplied block (which outlives the
/// process). `None` when absent or before `__torajs_argv_init` ran.
/// Each `envp[i]` is a NUL-terminated `"NAME=VALUE"`; match is exact
/// `NAME` length + byte compare.
fn env_find(name: &[u8]) -> Option<(*const u8, usize)> {
    if name.is_empty() {
        return None;
    }
    let envp_addr = ENVP_STATE.lock();
    if *envp_addr == 0 {
        return None;
    }
    let envp = *envp_addr as *const *const u8;
    drop(envp_addr);
    let mut i: usize = 0;
    loop {
        let entry: *const u8 = unsafe { *envp.add(i) };
        if entry.is_null() {
            return None;
        }
        // Find '=' within the entry; entry is guaranteed NUL-term.
        let mut eq = 0usize;
        loop {
            let b = unsafe { *entry.add(eq) };
            if b == b'=' || b == 0 {
                break;
            }
            eq += 1;
        }
        if eq == name.len()
            && unsafe { *entry.add(eq) } == b'='
            && unsafe { core::slice::from_raw_parts(entry, name.len()) } == name
        {
            // value starts after the '='
            let val_start = unsafe { entry.add(name.len() + 1) };
            let mut vlen = 0usize;
            while unsafe { *val_start.add(vlen) } != 0 {
                vlen += 1;
            }
            return Some((val_start, vlen));
        }
        i += 1;
    }
}

/// `process.env.NAME` — owned Str or NULL. Scans the `envp` block the
/// kernel passes to `main` (captured in `ENVP_STATE` by
/// `__torajs_argv_init`), avoiding the libc `getenv` / `_NSGetEnviron`
/// dyld dep.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_getenv(name_str: *const u8) -> *mut u8 {
    let nlen = unsafe { str_len(name_str) } as usize;
    if nlen == 0 {
        // Empty name never matches — same undefined answer as a
        // missing variable.
        return unsafe { __torajs_str_undef() };
    }
    let name = unsafe { core::slice::from_raw_parts(str_data(name_str), nlen) };
    match env_find(name) {
        Some((ptr, len)) => {
            let bytes = unsafe { core::slice::from_raw_parts(ptr, len) };
            unsafe { alloc_str(bytes) }
        }
        // Missing env var = JS undefined (the sentinel cell, RFC
        // 20260707 chunk 2), so `missing === undefined` holds.
        None => unsafe { __torajs_str_undef() },
    }
}

/// Runtime-internal raw env lookup (torajs-date's TZ probe): `name`
/// is bare bytes, NOT a tr Str. Returns a BORROWED pointer into the
/// envp block (lives for the process) and writes the value length
/// through `out_len`; NULL when the variable is absent or
/// `__torajs_argv_init` hasn't run.
///
/// # Safety
/// `name` points at `name_len` readable bytes; `out_len` is a valid
/// writable slot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_env_lookup_raw(
    name: *const u8,
    name_len: i64,
    out_len: *mut i64,
) -> *const u8 {
    if name.is_null() || name_len <= 0 {
        return core::ptr::null();
    }
    let name = unsafe { core::slice::from_raw_parts(name, name_len as usize) };
    match env_find(name) {
        Some((ptr, len)) => {
            unsafe { *out_len = len as i64 };
            ptr
        }
        None => core::ptr::null(),
    }
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

/// Stored `envp` pointer captured at LLVM-emitted `main` entry, used
/// by `__torajs_process_getenv` to walk environment without pulling
/// `_NSGetEnviron` / libc `getenv` into the user binary.
static ENVP_STATE: Mutex<usize> = Mutex::new(0);

/// `__torajs_argv_init(argc, argv, envp)` — main-entry plumbing.
/// Native exec ABI passes `(argc, argv, envp [, apple])` on the
/// stack; WASI passes `(argc, argv)` only and forwards a null `envp`
/// from the codegen entry. Both are stored as raw addresses (the
/// kernel-supplied stack frame outlives the process, so no copy or
/// lifetime gymnastics are needed).
///
/// # Safety
/// `argv` / `envp` must outlive the process. `envp` may be null
/// (WASI) — `__torajs_process_getenv` handles that case.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_argv_init(
    argc: i32,
    argv: *mut *mut c_char,
    envp: *mut *mut c_char,
) {
    {
        let mut state = ARGV_STATE.lock();
        *state = (argc, argv as usize);
    }
    {
        let mut e = ENVP_STATE.lock();
        *e = envp as usize;
    }
}

/// `process.argv` → fresh Array<Str>.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_argv() -> *mut u8 {
    let (argc, argv_addr) = {
        let state = ARGV_STATE.lock();
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

/// `process.stdout.write(s)` → bool. Via torajs-io's 0-libc
/// buffered writer (shared process-global line buffer with the
/// print family). Caller sees the write before the next syscall
/// via the explicit flush at the end.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_stdout_write(s: *const u8) -> bool {
    let dlen = unsafe { str_len(s) };
    let d = unsafe { str_data(s) };
    unsafe { __torajs_io_write_stdout(d, dlen) };
    unsafe { __torajs_io_flush() };
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
    // torajs-io's buffer so without this flush a redirected `2>&1`
    // would see stderr lines printed before still-buffered stdout
    // lines.
    unsafe { __torajs_io_flush() };
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
