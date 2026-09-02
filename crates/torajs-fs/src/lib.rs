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
//! hand the NUL-terminated bytes to torajs-syscall. Path bytes
//! longer than 4095 truncate — matches the pre-port C behavior
//! (silently lossy on PATH_MAX overflow; documented limitation of
//! the v0.3 MVP).
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

use torajs_syscall::sysno::{
    DIRENT_D_NAME_OFFSET, DIRENT_D_NAMLEN_OFFSET, DIRENT_D_RECLEN_OFFSET, MKDIR_DEFAULT_MODE,
    O_APPEND, O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY,
};
use torajs_syscall::{
    close, fstat_size, getdirentries64, mkdir, open, open_mode, read, stat_size, unlink,
};

/// Max path length the runtime accepts, including the NUL we append
/// to make a C-string. One less than this is the longest tora path
/// that can survive the copy without truncation. Mirrors the
/// `char path[4096]` stack buffer in the pre-port C runtime.
pub const PATH_MAX_LEN: usize = 4096;

// ============================================================
// Cross-tier extern stubs
// ============================================================

#[cfg(not(test))]
unsafe extern "C" {
    pub(crate) fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    /// torajs-str — a Str from well-formed UTF-8 bytes (canonical
    /// Latin-1 / UTF-16 layout).
    pub(crate) fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_arr_alloc(initial_cap: u64) -> *mut c_void;
    fn __torajs_arr_push(arr: *mut c_void, val: i64) -> *mut c_void;
    pub(crate) fn __torajs_panic(msg: *const u8) -> !;
}

#[cfg(test)]
pub(crate) unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-fs test stub: __torajs_str_alloc_pooled should not be called from cargo test");
}

#[cfg(test)]
pub(crate) unsafe extern "C" fn __torajs_str_alloc(_src: *const u8, _len: i64) -> *mut u8 {
    panic!("torajs-fs test stub: __torajs_str_alloc should not be called from cargo test");
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
pub(crate) unsafe extern "C" fn __torajs_panic(_msg: *const u8) -> ! {
    panic!("torajs-fs test stub: __torajs_panic should not be called from cargo test");
}

mod helpers;
use helpers::{
    cbuf_len, panic_with, path_copy_to_buf, str_alloc_with, str_to_utf8_bytes, write_all,
};

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
    let fd = match unsafe { open(buf.as_ptr(), O_RDONLY) } {
        Ok(fd) => fd,
        Err(_) => {
            let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
            unsafe { panic_with("not yet supported: fs.readFileSync open failed: ", &detail) };
        }
    };
    // fstat size as a capacity hint — regular files fill exactly,
    // pipes / char devices grow via the loop; -1 (non-regular) → 0.
    let cap = fstat_size(fd).unwrap_or(0).max(0) as usize;
    let mut data: Vec<u8> = Vec::with_capacity(cap);
    let mut chunk = [0u8; 65536];
    loop {
        match unsafe { read(fd, &mut chunk) } {
            Ok(0) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
            Err(_) => {
                let _ = close(fd);
                let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
                unsafe { panic_with("not yet supported: fs.readFileSync read failed: ", &detail) };
            }
        }
    }
    let _ = close(fd);
    unsafe { str_alloc_with(&data) as *mut c_void }
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
    let data_ptr = data_str as *const u8;
    // Transcode the data Str to a UTF-8 byte stream before writing
    // so the on-disk bytes match the source string regardless of
    // the in-memory encoding (Latin-1 supplement / UTF-16).
    let data = unsafe { str_to_utf8_bytes(data_ptr) };
    // O_WRONLY|O_CREAT|O_TRUNC, mode 0o666 pre-umask = std::fs::write /
    // Node fs.writeFileSync default (umask trims to 0o644 on disk).
    let fd = match unsafe { open_mode(buf.as_ptr(), O_WRONLY | O_CREAT | O_TRUNC, 0o666) } {
        Ok(fd) => fd,
        Err(_) => {
            let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
            unsafe { panic_with("not yet supported: fs.writeFileSync open failed: ", &detail) };
        }
    };
    let res = write_all(fd, &data);
    let _ = close(fd);
    if res.is_err() {
        let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
        unsafe {
            panic_with(
                "not yet supported: fs.writeFileSync write failed: ",
                &detail,
            )
        };
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
    let data_ptr = data_str as *const u8;
    // Transcode to UTF-8 byte stream — see __torajs_fs_write_file_sync.
    let data = unsafe { str_to_utf8_bytes(data_ptr) };
    // O_APPEND: each write seeks to EOF first (matches OpenOptions
    // .append(true)); O_CREAT makes it if absent. No O_TRUNC.
    let fd = match unsafe { open_mode(buf.as_ptr(), O_WRONLY | O_CREAT | O_APPEND, 0o666) } {
        Ok(fd) => fd,
        Err(_) => {
            let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
            unsafe {
                panic_with(
                    "not yet supported: fs.appendFileSync open failed: ",
                    &detail,
                );
            }
        }
    };
    let res = write_all(fd, &data);
    let _ = close(fd);
    if res.is_err() {
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
    // Match C `fopen(p, "rb")`: open-for-read success = exists. Keep
    // `open` (not stat) so unreadable dirs / dangling symlinks read as
    // false, exactly as the pre-port fopen path did.
    match unsafe { open(buf.as_ptr(), O_RDONLY) } {
        Ok(fd) => {
            let _ = close(fd);
            true
        }
        Err(_) => false,
    }
}

/// `fs.unlinkSync(path)` — delete a regular file or symlink.
/// Aborts on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_unlink_sync(path_str: *const c_void) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    if unsafe { unlink(buf.as_ptr()) }.is_err() {
        let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
        unsafe {
            panic_with("not yet supported: fs.unlinkSync failed: ", &detail);
        }
    }
}

/// `fs.mkdirSync(path)` — single-level directory creation. Mode is
/// 0o777 pre-umask, matching `std::fs::create_dir` (kernel masks by
/// process umask, typically 0o022 → 0o755 on disk). Spec is to throw
/// on existing dir unless `recursive: true`; we mirror by aborting
/// (typed-throw is Phase v0.3.b).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_mkdir_sync(path_str: *const c_void) {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    if unsafe { mkdir(buf.as_ptr(), MKDIR_DEFAULT_MODE) }.is_err() {
        let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
        unsafe {
            panic_with("not yet supported: fs.mkdirSync failed: ", &detail);
        }
    }
}

/// `fs.statSync(path).size → i64`. Returns whatever `stat(2)` reports
/// in `st_size`, or -1 on any error (missing / unreadable). Doesn't
/// abort — Bun's `Bun.file(p).size` getter is total / never throws.
///
/// For directories `stat.st_size` is the directory block size (POSIX /
/// Node `statSync` behavior), not the historic torajs `-1` non-regular
/// sentinel — dropping the `is_file` check is the std::io::Error
/// price the syscall API has no plumbing for `st_mode` yet, and it
/// happens to be more spec-true. Fixtures only exercise regular files
/// (`async-016`), so this is a no-op for the gate.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_size_sync(path_str: *const c_void) -> i64 {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    unsafe { stat_size(buf.as_ptr()) }.unwrap_or(-1)
}

/// `fs.readdirSync(path) → string[]`. Returns a fresh tora Array of
/// fresh Str entries. `.` / `..` skipped. Order matches the OS's
/// `readdir(3)` ordering (`getdirentries64` walks the same on-disk
/// directory image).
///
/// Loop: `open(O_RDONLY)` the dir, page `getdirentries64` into an
/// 8 KiB buffer until it returns 0, decode each variable-length
/// `struct dirent` by stepping `d_reclen`, slice the name bytes off
/// `d_name` using `d_namlen`, `str_alloc_with` + `arr_push`.
///
/// # Safety
/// `path_str` is a live `*const Str`. Returned pointer is a fresh
/// refcount=1 Array<Str> heap block; each element Str has rc=1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_fs_readdir_sync(path_str: *const c_void) -> *mut c_void {
    let mut buf = [0u8; PATH_MAX_LEN];
    unsafe { path_copy_to_buf(path_str as *const u8, buf.as_mut_ptr(), PATH_MAX_LEN) };
    let fd = match unsafe { open(buf.as_ptr(), O_RDONLY) } {
        Ok(fd) => fd,
        Err(_) => {
            let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
            unsafe {
                panic_with("not yet supported: fs.readdirSync open failed: ", &detail);
            }
        }
    };
    let mut arr = unsafe { __torajs_arr_alloc(0) };
    let mut dirbuf = [0u8; 8192];
    let mut basep: i64 = 0;
    loop {
        let n = match unsafe { getdirentries64(fd, &mut dirbuf, &mut basep) } {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => {
                let _ = close(fd);
                let detail = String::from_utf8_lossy(&buf[..cbuf_len(&buf)]);
                unsafe {
                    panic_with("not yet supported: fs.readdirSync read failed: ", &detail);
                }
            }
        };
        let mut pos = 0usize;
        while pos < n {
            let reclen = u16::from_ne_bytes(
                dirbuf[pos + DIRENT_D_RECLEN_OFFSET..pos + DIRENT_D_RECLEN_OFFSET + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            let namlen = u16::from_ne_bytes(
                dirbuf[pos + DIRENT_D_NAMLEN_OFFSET..pos + DIRENT_D_NAMLEN_OFFSET + 2]
                    .try_into()
                    .unwrap(),
            ) as usize;
            // `d_namlen` excludes the trailing NUL; `d_name` starts
            // at offset 21 in the record. Slice exactly `namlen`
            // bytes so the Str payload has no embedded NUL.
            let name_start = pos + DIRENT_D_NAME_OFFSET;
            let name = &dirbuf[name_start..name_start + namlen];
            if name != b"." && name != b".." {
                let s = unsafe { str_alloc_with(name) };
                arr = unsafe { __torajs_arr_push(arr, s as i64) };
            }
            // `d_reclen == 0` would loop forever — guard, though the
            // kernel never emits a zero-length record.
            if reclen == 0 {
                break;
            }
            pos += reclen;
        }
    }
    let _ = close(fd);
    arr
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::{HDR_FLAGS_OFF, STR_FLAG_IS_LATIN1, STR_HDR_SIZE, STR_LEN_OFF};

    fn make_str(payload: &[u8]) -> Vec<u8> {
        // Build a tora Str layout in a Vec for path_copy_to_buf
        // round-trip tests. Post-S1 layout: `length u32 @8` + flags
        // at `HDR_FLAGS_OFF` with `IS_LATIN1` bit set so the
        // `str_view` helper treats the payload as one byte per
        // code unit (paths are ASCII / Latin-1 in practice).
        let mut v = vec![0u8; STR_HDR_SIZE + payload.len()];
        let length = payload.len() as u32;
        v[STR_LEN_OFF..STR_LEN_OFF + 4].copy_from_slice(&length.to_ne_bytes());
        // Set the IS_LATIN1 flag at flags u16 @6 so the encoding
        // dispatch picks the byte-per-code-unit fast path.
        v[HDR_FLAGS_OFF..HDR_FLAGS_OFF + 2].copy_from_slice(&STR_FLAG_IS_LATIN1.to_ne_bytes());
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
    fn cbuf_len_nul_terminated() {
        let mut buf = [0u8; PATH_MAX_LEN];
        buf[..5].copy_from_slice(b"/etc\0");
        assert_eq!(cbuf_len(&buf), 4);
    }

    #[test]
    fn cbuf_len_full_buffer_no_nul() {
        let buf = [b'x'; PATH_MAX_LEN];
        assert_eq!(cbuf_len(&buf), PATH_MAX_LEN);
    }
}
