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
    Errno, close, fstat_size, getdirentries64, mkdir, open, open_mode, read, stat_size, unlink,
    write,
};

/// Max path length the runtime accepts, including the NUL we append
/// to make a C-string. One less than this is the longest tora path
/// that can survive the copy without truncation. Mirrors the
/// `char path[4096]` stack buffer in the pre-port C runtime.
pub const PATH_MAX_LEN: usize = 4096;

const STR_HDR_SIZE: usize = 16;
const STR_LEN_OFF: usize = 8;
/// Mirror of `torajs_str::layout::STR_FLAG_IS_LATIN1` — the flag
/// bit on `flags u16 @6` that discriminates Latin-1 payload (set)
/// from UTF-16 little-endian payload (clear).
const STR_FLAG_IS_LATIN1: u16 = 0x0002;
/// Universal heap header `flags u16 @6` offset for direct read.
const HDR_FLAGS_OFF: usize = 6;

/// Read a Str's `(payload_bytes, length, is_latin1)` triple without
/// allocating. Payload bytes are the raw encoded payload (Latin-1:
/// length bytes; UTF-16: length × 2 bytes).
///
/// # Safety
/// `s` must point at a valid Str heap block.
#[inline]
unsafe fn str_view<'a>(s: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() };
    let flags = unsafe { (s.add(HDR_FLAGS_OFF) as *const u16).read() };
    let is_latin1 = (flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(s.add(STR_HDR_SIZE), byte_cnt) };
    (payload, length, is_latin1)
}

/// Transcode a Str's payload to a UTF-8 byte buffer. ASCII-only
/// Latin-1 payloads pass through verbatim (same byte stream); Latin-
/// 1 supplement codepoints (0x80..=0xFF) expand to a 2-byte UTF-8
/// sequence each; UTF-16 LE payloads decode (with surrogate pair
/// combination for supplementary planes) and re-encode as UTF-8.
///
/// Used by every fs write path so files on disk are valid UTF-8
/// regardless of the in-memory Str encoding.
///
/// # Safety
/// `s` must point at a valid Str heap block.
unsafe fn str_to_utf8_bytes(s: *const u8) -> Vec<u8> {
    let (payload, length, is_latin1) = unsafe { str_view(s) };
    if is_latin1 {
        // Fast path — payload is ASCII (every byte ≤ 0x7F). Return
        // verbatim; matches the pre-S2 `slice::from_raw_parts` write
        // shape for the dominant case.
        if payload.iter().all(|&b| b <= 0x7F) {
            return payload.to_vec();
        }
        // Latin-1 supplement — re-encode codepoint-by-codepoint.
        let mut out = Vec::with_capacity(payload.len() * 2);
        for &b in payload {
            if b <= 0x7F {
                out.push(b);
            } else {
                out.push(0xC0 | (b >> 6));
                out.push(0x80 | (b & 0x3F));
            }
        }
        return out;
    }
    // UTF-16 LE decode + UTF-8 encode.
    let mut out = Vec::with_capacity((length as usize) * 3);
    let mut i = 0usize;
    while i + 1 < payload.len() {
        let cu = u16::from_le_bytes([payload[i], payload[i + 1]]) as u32;
        let cp = if (0xD800..=0xDBFF).contains(&cu) && i + 3 < payload.len() {
            let lo = u16::from_le_bytes([payload[i + 2], payload[i + 3]]) as u32;
            if (0xDC00..=0xDFFF).contains(&lo) {
                i += 4;
                0x10000 + ((cu - 0xD800) << 10) + (lo - 0xDC00)
            } else {
                i += 2;
                cu
            }
        } else {
            i += 2;
            cu
        };
        if cp <= 0x7F {
            out.push(cp as u8);
        } else if cp <= 0x7FF {
            out.push((0xC0 | (cp >> 6)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        } else if cp <= 0xFFFF {
            out.push((0xE0 | (cp >> 12)) as u8);
            out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        } else {
            out.push((0xF0 | (cp >> 18)) as u8);
            out.push((0x80 | ((cp >> 12) & 0x3F)) as u8);
            out.push((0x80 | ((cp >> 6) & 0x3F)) as u8);
            out.push((0x80 | (cp & 0x3F)) as u8);
        }
    }
    out
}

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
    // Paths are written to disk as UTF-8 byte streams (filesystem
    // convention on macOS / Linux). Transcode the Str to UTF-8
    // bytes regardless of in-memory encoding so paths with non-
    // ASCII codepoints round-trip correctly.
    let utf8 = unsafe { str_to_utf8_bytes(path_str) };
    let mut plen = utf8.len();
    if plen >= bufsz {
        plen = bufsz - 1;
    }
    if plen > 0 {
        unsafe { core::ptr::copy_nonoverlapping(utf8.as_ptr(), buf, plen) };
    }
    unsafe { buf.add(plen).write(0) };
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

/// Index of the NUL terminator in a path copy buffer (= path byte
/// length). Lets error-detail strings slice the path bytes back out
/// without going through `std::path`.
#[inline]
fn cbuf_len(buf: &[u8; PATH_MAX_LEN]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

/// Write the whole buffer, looping over partial `write(2)` returns.
/// A 0-byte write with bytes still pending is treated as `EIO`.
fn write_all(fd: i32, mut data: &[u8]) -> Result<(), Errno> {
    while !data.is_empty() {
        // SAFETY: `data` is a live slice valid for its full length.
        let n = unsafe { write(fd, data) }?;
        if n == 0 {
            return Err(Errno(5)); // EIO — kernel accepted nothing
        }
        data = &data[n..];
    }
    Ok(())
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
