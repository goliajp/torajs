//! Fatal-error helper for the torajs AOT TypeScript runtime.
//!
//! Layer-1 substrate (P7.i-panic, 2026-05-24) — replaces the
//! `__torajs_panic` in `runtime_str.c`. Single extern fn:
//!
//! ```text
//! #[unsafe(no_mangle)]
//! pub unsafe extern "C" fn __torajs_panic(msg: *const u8) -> !
//! ```
//!
//! ## Behavior
//!
//! 1. Writes the NUL-terminated message + `\n` to stderr.
//! 2. If the message starts with `"not yet supported:"` (the
//!    conformance / test262 substrate-boundary marker), skips the
//!    backtrace — emitting one would shift the case from the
//!    "incompatible" bucket to the "true crash" bucket.
//! 3. Otherwise captures up to 32 frames via libc `backtrace(3)`:
//!    - macOS: shells out to `atos -o <self_path> -arch arm64 <PCs>`
//!      to symbolicate frames against the binary's `.dSYM`. PCs are
//!      converted to STATIC addresses by subtracting the ASLR slide
//!      (`_dyld_get_image_vmaddr_slide(0)`), which the recent
//!      macOS atos `-l <slide>` flag has been unreliable about.
//!    - Linux: prints raw `0xPC (in <self_path>)` lines; user runs
//!      `addr2line -e <self_path> <pc>` to symbolicate.
//! 4. Calls libc `exit(1)`. Never returns.
//!
//! ## wasm32-wasi
//!
//! WASI has no `backtrace(3)` / `_NSGetExecutablePath` / atos. The
//! `#[cfg(target_os = "wasi")]` path degrades to `fputs + exit(1)`.

use core::ffi::{c_char, c_int, c_void};

const STDERR_FILENO: i32 = 2;

unsafe extern "C" {
    fn write(fd: i32, buf: *const c_void, n: usize) -> isize;
    fn exit(code: i32) -> !;
    fn strlen(s: *const u8) -> usize;
    fn strncmp(a: *const u8, b: *const u8, n: usize) -> i32;
}

#[cfg(not(target_os = "wasi"))]
unsafe extern "C" {
    fn backtrace(frames: *mut *mut c_void, size: c_int) -> c_int;
    fn snprintf(buf: *mut c_char, n: usize, fmt: *const u8, ...) -> i32;
    fn system(cmd: *const c_char) -> i32;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn _NSGetExecutablePath(buf: *mut c_char, size: *mut u32) -> i32;
    fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn readlink(path: *const c_char, buf: *mut c_char, size: usize) -> isize;
}

// "not yet supported:" prefix — see module docs.
const SUPPRESS_BT_PREFIX: &[u8] = b"not yet supported:";

#[inline]
unsafe fn write_all_stderr(buf: &[u8]) {
    let mut p = buf.as_ptr() as *const c_void;
    let mut n = buf.len();
    while n > 0 {
        let w = unsafe { write(STDERR_FILENO, p, n) };
        if w <= 0 {
            return;
        }
        let w = w as usize;
        p = unsafe { (p as *const u8).add(w) as *const c_void };
        n -= w;
    }
}

#[cfg(not(target_os = "wasi"))]
fn self_path(buf: &mut [u8]) -> &[u8] {
    #[cfg(target_os = "macos")]
    {
        let mut sz = buf.len() as u32;
        let rc = unsafe { _NSGetExecutablePath(buf.as_mut_ptr() as *mut c_char, &mut sz) };
        if rc == 0 {
            // NUL-terminated; find length.
            let len = unsafe { strlen(buf.as_ptr()) };
            return &buf[..len];
        }
    }
    #[cfg(target_os = "linux")]
    {
        let path = b"/proc/self/exe\0";
        let n = unsafe {
            readlink(
                path.as_ptr() as *const c_char,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() - 1,
            )
        };
        if n > 0 {
            buf[n as usize] = 0;
            return &buf[..n as usize];
        }
    }
    buf[0] = b'?';
    buf[1] = 0;
    &buf[..1]
}

/// `__torajs_panic(msg)` — central fatal-error handler. See module
/// docs for the full behavior contract.
///
/// # Safety
/// `msg` must be a NUL-terminated C string (any pointer ssa_lower
/// emits at a panic site).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_panic(msg: *const u8) -> ! {
    // Step 1 — write message + newline to stderr.
    let msg_len = unsafe { strlen(msg) };
    unsafe {
        write_all_stderr(core::slice::from_raw_parts(msg, msg_len));
        write_all_stderr(b"\n");
    }

    // Step 2 — early exit on WASI (no backtrace facility).
    #[cfg(target_os = "wasi")]
    unsafe {
        exit(1)
    }

    #[cfg(not(target_os = "wasi"))]
    unsafe {
        // Step 3 — capture frames + symbolicate (unless suppressed).
        let suppress = msg_len >= SUPPRESS_BT_PREFIX.len()
            && strncmp(msg, SUPPRESS_BT_PREFIX.as_ptr(), SUPPRESS_BT_PREFIX.len()) == 0;
        let mut frames: [*mut c_void; 32] = [core::ptr::null_mut(); 32];
        let n = if suppress {
            0
        } else {
            backtrace(frames.as_mut_ptr(), 32)
        };
        if n > 1 {
            let mut path_buf = [0u8; 4096];
            let path = self_path(&mut path_buf);
            write_all_stderr(b"backtrace:\n");
            #[cfg(target_os = "macos")]
            {
                emit_macos_backtrace(path, &frames[..n as usize]);
            }
            #[cfg(not(target_os = "macos"))]
            {
                emit_raw_backtrace(path, &frames[..n as usize]);
            }
        }
        exit(1)
    }
}

#[cfg(target_os = "macos")]
unsafe fn emit_macos_backtrace(self_path: &[u8], frames: &[*mut c_void]) {
    // `atos -o <self> -arch arm64 <static_pc1> <static_pc2> ... 1>&2`
    // static_pc = runtime_pc - aslr_slide. atos `-l` flag has been
    // unreliable for arm64; subtract slide ourselves.
    let slide = unsafe { _dyld_get_image_vmaddr_slide(0) };
    let mut cmd = [0u8; 8192];
    // Open quote for the binary path: atos -o '<self>' -arch arm64
    // We assume the path doesn't contain ' — fine for typical /tmp
    // and ~/<*> paths. Pre-port C had the same assumption.
    let mut off: i32 = unsafe {
        snprintf(
            cmd.as_mut_ptr() as *mut c_char,
            cmd.len(),
            b"atos -o '%.*s' -arch arm64\0".as_ptr(),
            self_path.len() as i32,
            self_path.as_ptr(),
        )
    };
    if off < 0 {
        return;
    }
    // Skip frame 0 (the call to __torajs_panic itself).
    for &f in frames.iter().skip(1) {
        if off as usize > cmd.len() - 32 {
            break;
        }
        let static_pc = (f as isize - slide) as usize;
        let r = unsafe {
            snprintf(
                cmd.as_mut_ptr().add(off as usize) as *mut c_char,
                cmd.len() - off as usize,
                b" 0x%lx\0".as_ptr(),
                static_pc,
            )
        };
        if r < 0 {
            return;
        }
        off += r;
    }
    let _ = unsafe {
        snprintf(
            cmd.as_mut_ptr().add(off as usize) as *mut c_char,
            cmd.len() - off as usize,
            b" 1>&2\0".as_ptr(),
        )
    };
    unsafe { system(cmd.as_ptr() as *const c_char) };
}

#[cfg(all(not(target_os = "macos"), not(target_os = "wasi")))]
unsafe fn emit_raw_backtrace(self_path: &[u8], frames: &[*mut c_void]) {
    // Raw PCs. User can resolve via `addr2line -e <self_path> <pc>`.
    let mut buf = [0u8; 256];
    for &f in frames.iter().skip(1) {
        let n = unsafe {
            snprintf(
                buf.as_mut_ptr() as *mut c_char,
                buf.len(),
                b"  %p (in %.*s)\n\0".as_ptr(),
                f,
                self_path.len() as i32,
                self_path.as_ptr(),
            )
        };
        if n > 0 {
            unsafe { write_all_stderr(core::slice::from_raw_parts(buf.as_ptr(), n as usize)) };
        }
    }
}
