//! Synchronous filesystem substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-3 substrate (P7.d, 2026-05-24) — replaces the `fs_*`
//! family in `runtime_str.c`. Covers the v0.3 `fs` module surface:
//!
//! - `readFileSync(path) → string` — whole-file read into a fresh Str
//! - `writeFileSync(path, data)` — whole-file write (truncates)
//! - `appendFileSync(path, data)` — append-mode write
//! - `existsSync(path) → boolean`
//! - `unlinkSync(path)` — `unlink(2)`
//! - `mkdirSync(path)` — `mkdir(2)`, mode 0755, single-level (no `recursive`)
//! - `statSync(path).size → i64` — file size, -1 on missing / non-regular
//! - `readdirSync(path) → string[]` — directory entries (excludes `.` / `..`)
//!
//! ## Path bytes
//!
//! Paths arrive as tora `Str` heap blocks — `len:u64` at offset 8,
//! payload at offset 16, NOT NUL-terminated. We copy onto a stack
//! buffer (`PATH_MAX` = 4096 with one byte reserved for NUL) and
//! pass to libc / std::fs via `Path::new`. Path bytes longer than
//! 4095 truncate — matches the pre-port C behavior (silently lossy
//! on PATH_MAX overflow; documented limitation of the v0.3 MVP).
//!
//! ## Error model
//!
//! Every fallible op aborts via [`extern_call::panic`] with a
//! `"not yet supported: ..."` message, identical wording to the
//! pre-port C runtime. Typed throw integration is Phase v0.3.b
//! (after `torajs-throw` substrate stabilizes for cross-tier use).
//!
//! ## Cross-tier ABI
//!
//! Calls into other sub-crates at `tr build` link time:
//! - `__torajs_str_alloc_pooled(len)` from `torajs-str`
//! - `__torajs_arr_alloc(initial_cap)` + `__torajs_arr_push(arr, val)`
//!   from `torajs-arr` (readdir result accumulator)
//! - `__torajs_panic(msg)` from `runtime_str.c` (will move to a
//!   `torajs-panic` crate in a later phase)
//!
//! `cargo test -p torajs-fs` substitutes panicking stubs for these
//! symbols — torajs-fs unit tests only exercise the path-copy /
//! buffer-handling logic that doesn't touch the cross-tier surface.

use core::ffi::c_void;

/// Max path length the runtime accepts, including the NUL we append
/// to make a C-string. One less than this is the longest tora path
/// that can survive the copy without truncation. Mirrors the
/// `char path[4096]` stack buffer in the pre-port C runtime.
pub const PATH_MAX_LEN: usize = 4096;

const STR_HDR_SIZE: usize = 16;
const STR_LEN_OFF: usize = 8;

// ============================================================
// Cross-tier extern stubs
// ============================================================

#[cfg(not(test))]
unsafe extern "C" {
    fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    fn __torajs_panic(msg: *const u8) -> !;
}

#[cfg(test)]
unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-fs test stub: __torajs_str_alloc_pooled should not be called from cargo test");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_arr_alloc(_cap: u64) -> *mut c_void {
    panic!("torajs-fs test stub: __torajs_arr_alloc should not be called from cargo test");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_arr_push(_arr: *mut c_void, _val: i64) -> *mut c_void {
    panic!("torajs-fs test stub: __torajs_arr_push should not be called from cargo test");
}

#[cfg(test)]
unsafe extern "C" fn __torajs_panic(_msg: *const u8) -> ! {
    panic!("torajs-fs test stub: __torajs_panic should not be called from cargo test");
}

// ============================================================
// Helpers
// ============================================================

/// Copy a tora Str's payload into a stack-allocated C-string-style
/// buffer (`buf[0..len] = payload; buf[len] = 0`). Truncates to
/// `bufsz - 1` if the Str is longer.
///
/// # Safety
/// `path_str` must be a valid `*const Str` (live, rc > 0). `buf`
/// must point at a writable region of at least `bufsz` bytes.
#[inline]
unsafe fn path_copy_to_buf(path_str: *const u8, buf: *mut u8, bufsz: usize) {
    let p = unsafe { path_str.add(STR_HDR_SIZE) };
    let mut plen = unsafe { (path_str.add(STR_LEN_OFF) as *const u64).read() } as usize;
    if plen >= bufsz {
        plen = bufsz - 1;
    }
    if plen > 0 {
        unsafe { core::ptr::copy_nonoverlapping(p, buf, plen) };
    }
    unsafe { buf.add(plen).write(0) };
}

/// Build a `&Path` from the NUL-terminated buffer the C-style
/// `path_copy_to_buf` produced. The borrow is valid as long as
/// `buf` is.
#[inline]
unsafe fn buf_as_path(buf: &[u8; PATH_MAX_LEN]) -> &std::path::Path {
    // Find the NUL we wrote; on truncation it's at `bufsz - 1`.
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let bytes = &buf[..nul];
    // SAFETY: std::path::Path accepts arbitrary bytes via OsStr on
    // POSIX. macOS / Linux both treat paths as bytes — no UTF-8
    // requirement, matches the pre-port C runtime's byte-level
    // handling. On Windows this would need OsStr::from_wide; we
    // don't target Windows.
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    std::path::Path::new(OsStr::from_bytes(bytes))
}

/// Abort with a `"not yet supported: ..."` message routed through
/// `__torajs_panic`. The message is a NUL-terminated heap-allocated
/// C-string; we use a single owned `Vec<u8>` per call site to keep
/// the formatter simple.
#[inline]
unsafe fn panic_with(prefix: &str, op_detail: &str) -> ! {
    let mut msg = Vec::with_capacity(prefix.len() + op_detail.len() + 1);
    msg.extend_from_slice(prefix.as_bytes());
    msg.extend_from_slice(op_detail.as_bytes());
    msg.push(0);
    unsafe { __torajs_panic(msg.as_ptr()) }
}

/// Allocate a Str with `data` as payload. The data must outlive the
/// `Self::alloc` call; the call copies bytes into the fresh block.
#[inline]
unsafe fn str_alloc_with(data: &[u8]) -> *mut u8 {
    let s = unsafe { __torajs_str_alloc_pooled(data.len() as u64) };
    if !data.is_empty() {
        unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), s.add(STR_HDR_SIZE), data.len()) };
    }
    s
}

// ============================================================
// fs.readFileSync / writeFileSync / appendFileSync
// ============================================================

/// `fs.readFileSync(path) → string`. Reads the whole file into a
/// fresh pooled Str (refcount = 1). Aborts on open / read failure.
///
/// # Safety
/// `path_str` is a live `*const Str` whose payload bytes form a
/// POSIX path. Returned pointer is a fresh refcount=1 Str heap
/// block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_read_file_sync(path_str: *const c_void) -> *mut c_void {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    match std::fs::read(path) {
        Ok(bytes) => unsafe { str_alloc_with(&bytes) as *mut c_void },
        Err(_) => {
            let detail = path.to_string_lossy();
            unsafe { panic_with("not yet supported: fs.readFileSync open failed: ", &detail) };
        }
    }
}

/// `fs.writeFileSync(path, data)` — overwrite-mode write. Aborts
/// on open / short-write failure.
///
/// # Safety
/// `path_str` and `data_str` are live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_write_file_sync(
    path_str: *const c_void,
    data_str: *const c_void,
) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    let data_ptr = data_str as *const u8;
    let dlen = unsafe { (data_ptr.add(STR_LEN_OFF) as *const u64).read() } as usize;
    let data = unsafe { core::slice::from_raw_parts(data_ptr.add(STR_HDR_SIZE), dlen) };
    if let Err(_) = std::fs::write(path, data) {
        let detail = path.to_string_lossy();
        unsafe { panic_with("not yet supported: fs.writeFileSync open failed: ", &detail) };
    }
}

/// `fs.appendFileSync(path, data)` — append-mode write. Creates
/// the file if it does not exist. Aborts on open / short-write
/// failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_append_file_sync(
    path_str: *const c_void,
    data_str: *const c_void,
) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    let data_ptr = data_str as *const u8;
    let dlen = unsafe { (data_ptr.add(STR_LEN_OFF) as *const u64).read() } as usize;
    let data = unsafe { core::slice::from_raw_parts(data_ptr.add(STR_HDR_SIZE), dlen) };
    use std::io::Write;
    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        Err(_) => {
            let detail = path.to_string_lossy();
            unsafe {
                panic_with(
                    "not yet supported: fs.appendFileSync open failed: ",
                    &detail,
                );
            }
        }
    };
    if f.write_all(data).is_err() {
        unsafe {
            panic_with("not yet supported: fs.appendFileSync short write", "");
        }
    }
}

// ============================================================
// fs.existsSync / unlinkSync / mkdirSync / statSync.size /
// readdirSync
// ============================================================

/// `fs.existsSync(path) → boolean`. Does not abort on any error —
/// missing / permission-denied / non-regular all return `false`,
/// matching the pre-port `fopen(..., "rb")` semantics where any
/// failure is "doesn't exist for read purposes".
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_exists_sync(path_str: *const c_void) -> bool {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    // Match C `fopen(p, "rb")`: open-for-read success = exists. We
    // can't use `Path::exists()` because it returns true for unreadable
    // dirs / dangling symlinks where fopen would fail.
    std::fs::File::open(path).is_ok()
}

/// `fs.unlinkSync(path)` — delete a regular file or symlink.
/// Aborts on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_unlink_sync(path_str: *const c_void) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    if std::fs::remove_file(path).is_err() {
        let detail = path.to_string_lossy();
        unsafe {
            panic_with("not yet supported: fs.unlinkSync failed: ", &detail);
        }
    }
}

/// `fs.mkdirSync(path)` — single-level directory creation with
/// permissions 0755 (libc default; std::fs::create_dir uses the
/// process umask). Spec is to throw on existing dir unless
/// `recursive: true`; we mirror by aborting (typed-throw is
/// Phase v0.3.b).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_mkdir_sync(path_str: *const c_void) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    if std::fs::create_dir(path).is_err() {
        let detail = path.to_string_lossy();
        unsafe {
            panic_with("not yet supported: fs.mkdirSync failed: ", &detail);
        }
    }
}

/// `fs.statSync(path).size → i64`. Returns the file's byte length,
/// or -1 on any error (missing / unreadable / non-regular). Doesn't
/// abort — Bun's `Bun.file(p).size` getter is total / never throws.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_size_sync(path_str: *const c_void) -> i64 {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    let Ok(meta) = std::fs::metadata(path) else {
        return -1;
    };
    if !meta.is_file() {
        return -1;
    }
    meta.len() as i64
}

/// `fs.readdirSync(path) → string[]`. Returns a fresh tora Array
/// of fresh Str entries. `.` / `..` skipped. Order matches the OS's
/// `readdir(3)` ordering.
///
/// # Safety
/// `path_str` is a live `*const Str`. Returned pointer is a fresh
/// refcount=1 Array<Str> heap block; each element Str has rc=1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_readdir_sync(path_str: *const c_void) -> *mut c_void {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let path = unsafe { buf_as_path(&buf) };
    let read_dir = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(_) => {
            let detail = path.to_string_lossy();
            unsafe {
                panic_with("not yet supported: fs.readdirSync open failed: ", &detail);
            }
        }
    };
    let mut arr = unsafe { __torajs_arr_alloc(0) };
    use std::os::unix::ffi::OsStrExt;
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let bytes = name.as_bytes();
        // Spec-skip `.` and `..` — std::fs::read_dir already drops
        // them on POSIX, but be defensive (matches the pre-port
        // C runtime's explicit check).
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let s = unsafe { str_alloc_with(bytes) };
        arr = unsafe { __torajs_arr_push(arr, s as i64) };
    }
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_str(payload: &[u8]) -> Vec<u8> {
        // Build a tora Str layout in a Vec for path_copy_to_buf
        // round-trip tests. Header bytes are uninitialized — only
        // `len` at offset 8 + payload at offset 16 matter for the
        // path-copy code path.
        let mut v = vec![0u8; STR_HDR_SIZE + payload.len()];
        let len = payload.len() as u64;
        v[STR_LEN_OFF..STR_LEN_OFF + 8].copy_from_slice(&len.to_ne_bytes());
        v[STR_HDR_SIZE..].copy_from_slice(payload);
        v
    }

    #[test]
    fn path_copy_short_path() {
        let s = make_str(b"/tmp/foo");
        let mut buf = [0u8; 32];
        unsafe { path_copy_to_buf(s.as_ptr(), buf.as_mut_ptr(), 32) };
        assert_eq!(&buf[..8], b"/tmp/foo");
        assert_eq!(buf[8], 0);
    }

    #[test]
    fn path_copy_truncates_at_bufsz_minus_one() {
        let long = vec![b'x'; 100];
        let s = make_str(&long);
        let mut buf = [0u8; 16];
        unsafe { path_copy_to_buf(s.as_ptr(), buf.as_mut_ptr(), 16) };
        // First 15 bytes are 'x', last byte is the NUL terminator.
        for &b in &buf[..15] {
            assert_eq!(b, b'x');
        }
        assert_eq!(buf[15], 0);
    }

    #[test]
    fn path_copy_empty_path() {
        let s = make_str(b"");
        let mut buf = [0u8; 8];
        unsafe { path_copy_to_buf(s.as_ptr(), buf.as_mut_ptr(), 8) };
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn buf_as_path_round_trip() {
        let mut buf = [0u8; PATH_MAX_LEN];
        buf[..5].copy_from_slice(b"/etc\0");
        let p = unsafe { buf_as_path(&buf) };
        assert_eq!(p.to_str(), Some("/etc"));
    }
}
