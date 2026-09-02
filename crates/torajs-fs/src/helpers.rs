//! Cross-cutting helpers shared by every `fs.*Sync` FFI shim:
//! Str payload decoding, UTF-8 transcoding, path buffer copy,
//! panic routing, fresh Str alloc, NUL terminator find, and the
//! partial-write loop. Extracted from `lib.rs` to keep that file
//! under the 500-prod-LOC file-size hard limit (`rules/common/
//! file-size.md`). Pure mechanical pull, no semantic change.

use torajs_syscall::{Errno, write};

use crate::{__torajs_panic, __torajs_str_alloc, __torajs_str_alloc_pooled, PATH_MAX_LEN};

pub(crate) const STR_HDR_SIZE: usize = 16;
pub(crate) const STR_LEN_OFF: usize = 8;
/// Mirror of `torajs_str::layout::STR_FLAG_IS_LATIN1` — the flag
/// bit on `flags u16 @6` that discriminates Latin-1 payload (set)
/// from UTF-16 little-endian payload (clear).
pub(crate) const STR_FLAG_IS_LATIN1: u16 = 0x0002;
/// Universal heap header `flags u16 @6` offset for direct read.
pub(crate) const HDR_FLAGS_OFF: usize = 6;

/// Read a Str's `(payload_bytes, length, is_latin1)` triple without
/// allocating. Payload bytes are the raw encoded payload (Latin-1:
/// length bytes; UTF-16: length × 2 bytes).
///
/// # Safety
/// `s` must point at a valid Str heap block.
#[inline]
pub(crate) unsafe fn str_view<'a>(s: *const u8) -> (&'a [u8], u32, bool) {
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
pub(crate) unsafe fn str_to_utf8_bytes(s: *const u8) -> Vec<u8> {
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
        // A lone surrogate has no UTF-8 spelling; the file gets
        // U+FFFD, as bun (TextEncoder semantics) writes it (558-05).
        let cp = if (0xD800..=0xDFFF).contains(&cp) {
            0xFFFD
        } else {
            cp
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

/// Copy a tora Str's payload into a stack-allocated C-string-style
/// buffer (`buf[0..len] = payload; buf[len] = 0`). Truncates to
/// `bufsz - 1` if the Str is longer.
///
/// # Safety
/// `path_str` must be a valid `*const Str` (live, rc > 0). `buf`
/// must point at a writable region of at least `bufsz` bytes.
#[inline]
pub(crate) unsafe fn path_copy_to_buf(path_str: *const u8, buf: *mut u8, bufsz: usize) {
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
pub(crate) unsafe fn panic_with(prefix: &str, op_detail: &str) -> ! {
    let mut msg = Vec::with_capacity(prefix.len() + op_detail.len() + 1);
    msg.extend_from_slice(prefix.as_bytes());
    msg.extend_from_slice(op_detail.as_bytes());
    msg.push(0);
    unsafe { __torajs_panic(msg.as_ptr()) }
}

/// A Str holding `data` read off the filesystem. Well-formed UTF-8
/// (the `utf8` read of a text file, a directory entry name) decodes
/// into the canonical Latin-1 / UTF-16 layout — the string value is
/// the file's characters, not its bytes (559-06: `"é"` read back
/// as the two-unit `"Ã©"`). Anything else is kept byte for byte in
/// the Latin-1 layout, the shape a binary read has always had.
#[inline]
pub(crate) unsafe fn str_alloc_with(data: &[u8]) -> *mut u8 {
    if core::str::from_utf8(data).is_ok() {
        return unsafe { __torajs_str_alloc(data.as_ptr(), data.len() as i64) };
    }
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
pub(crate) fn cbuf_len(buf: &[u8; PATH_MAX_LEN]) -> usize {
    buf.iter().position(|&b| b == 0).unwrap_or(buf.len())
}

/// Write the whole buffer, looping over partial `write(2)` returns.
/// A 0-byte write with bytes still pending is treated as `EIO`.
pub(crate) fn write_all(fd: i32, mut data: &[u8]) -> Result<(), Errno> {
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
