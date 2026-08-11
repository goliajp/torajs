//! Str console print — the `console.log(str)` SSA dispatch target
//! plus the stderr diagnostics writer.
//!
//! **Buffer-sharing constraint**: `__torajs_str_print`
//! uses `torajs_io::__torajs_io_putc_out` per-byte (v0.7-A3
//! Step 14-b cutover from libc `putchar`). torajs-print's
//! `print_i64` / `print_f64` / `print_bool` also route through
//! the same symbol (Step 14-c cutover), so all print writers
//! share torajs-io's process-
//! global line buffer. No cross-buffer reordering risk: every
//! console.log call ends with '\n' which triggers a flush.
//!
//! `console.error(str)` no longer has a dedicated `_err` twin here:
//! since RFC 20260812-console-sink knife 2 the lowering brackets the
//! ordinary `__torajs_str_print` call with torajs-io's current-sink
//! switch, so the same streaming path serves both streams. The
//! remaining stderr-direct writer is [`__torajs_str_write_err`]
//! (the unhandled-rejection reporter's composing primitive).
//!
//! ## P11.1-S2.1 encoding-aware output
//!
//! Pre-S2 the Str payload was raw UTF-8 bytes, so all three print
//! helpers wrote payload bytes verbatim to stdout / stderr — the
//! terminal already speaks UTF-8 and the round-trip was bit-
//! identical to the source code. Post-S2 the payload is either
//! Latin-1 (one byte per code unit, 0..=0xFF) or UTF-16 little-
//! endian (two bytes per code unit with surrogate pair encoding
//! for codepoints > 0xFFFF). Neither is a UTF-8 byte stream the
//! terminal can render directly — so the print helpers now decode
//! the encoded payload into Unicode codepoints and re-encode each
//! as UTF-8 before handing bytes off to putchar / write_stderr.
//!
//! Latin-1 fast path: codepoints ≤ 0x7F are written verbatim
//! (ASCII subset of UTF-8); codepoints 0x80..=0xFF expand to a
//! 2-byte UTF-8 sequence inline. ASCII-only payloads stay
//! one-byte-per-codepoint — same write cost as the pre-S2 path.
//!
//! UTF-16 path: pairs of bytes are read as little-endian u16,
//! surrogate pairs combined to a single supplementary-plane
//! codepoint, then re-encoded as UTF-8 (1-4 bytes per codepoint).
//! Lone surrogates pass through as their numeric value (ES spec
//! allows non-well-formed UTF-16 in JS strings; the terminal will
//! see the lone surrogate as a 3-byte UTF-8 sequence even though
//! it's not strictly valid Unicode — this matches V8 / JSC
//! behavior when console.log'ing lone surrogates).

use alloc::vec::Vec;

use torajs_rc::HeapHeader;

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::substr::{SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF};

// ============================================================
// Encoding helpers — pure Rust, no I/O
// ============================================================

/// Encode a single Unicode code point (0..=0x10FFFF) as a UTF-8
/// byte sequence, feeding each byte to `put`. Inlined so the
/// per-byte putchar call site stays a single function-call cost
/// instead of one putchar per UTF-8 byte plus the helper's own
/// call.
#[inline]
pub(crate) fn write_utf8_for_codepoint<F: FnMut(u8)>(cp: u32, mut put: F) {
    if cp <= 0x7F {
        put(cp as u8);
    } else if cp <= 0x7FF {
        put((0xC0 | (cp >> 6)) as u8);
        put((0x80 | (cp & 0x3F)) as u8);
    } else if cp <= 0xFFFF {
        put((0xE0 | (cp >> 12)) as u8);
        put((0x80 | ((cp >> 6) & 0x3F)) as u8);
        put((0x80 | (cp & 0x3F)) as u8);
    } else {
        // cp ≤ 0x10FFFF per Unicode; supplementary plane.
        put((0xF0 | (cp >> 18)) as u8);
        put((0x80 | ((cp >> 12) & 0x3F)) as u8);
        put((0x80 | ((cp >> 6) & 0x3F)) as u8);
        put((0x80 | (cp & 0x3F)) as u8);
    }
}

/// Walk a UTF-16 LE encoded payload (`bytes.len()` must be even),
/// resolving surrogate pairs to single supplementary-plane code
/// points, and feed each decoded code point to `yield_cp`.
///
/// Lone surrogates (a high surrogate not followed by a low one,
/// or a low surrogate appearing in isolation) are yielded as
/// their u16 numeric value — per ES spec, JS strings allow
/// non-well-formed UTF-16 and downstream consumers should
/// preserve the bits.
#[inline]
pub(crate) fn iter_utf16_codepoints(bytes: &[u8], mut yield_cp: impl FnMut(u32)) {
    let mut i = 0;
    while i + 1 < bytes.len() {
        let cu = u16::from_le_bytes([bytes[i], bytes[i + 1]]) as u32;
        if (0xD800..=0xDBFF).contains(&cu) && i + 3 < bytes.len() {
            let lo = u16::from_le_bytes([bytes[i + 2], bytes[i + 3]]) as u32;
            if (0xDC00..=0xDFFF).contains(&lo) {
                let cp = 0x10000 + ((cu - 0xD800) << 10) + (lo - 0xDC00);
                yield_cp(cp);
                i += 4;
                continue;
            }
        }
        // Lone surrogate or BMP code unit — yield as-is.
        yield_cp(cu);
        i += 2;
    }
}

/// Encode a Latin-1 payload byte as a UTF-8 byte sequence
/// (1 byte if ≤ 0x7F, 2 bytes otherwise). Wrapper around
/// [`write_utf8_for_codepoint`] for the common Latin-1 path
/// where the byte value is also the codepoint value.
#[inline]
fn write_utf8_for_latin1_byte<F: FnMut(u8)>(b: u8, put: F) {
    write_utf8_for_codepoint(b as u32, put);
}

/// Read the `IS_LATIN1` bit out of a Str (or Substr / parent-Str)
/// heap header's flags field. The header lives at offset 0 with
/// the universal `{refcount u32 @0, type_tag u16 @4, flags u16 @6}`
/// layout, so flags-at-bit-1 is the cross-payload encoding flag.
///
/// # Safety
///
/// `p` must point at a valid Str or Substr block whose first 8
/// bytes are the universal heap header.
#[inline]
unsafe fn header_is_latin1(p: *const u8) -> bool {
    let header = unsafe { &*(p as *const HeapHeader) };
    (header.flags & STR_FLAG_IS_LATIN1) != 0
}

// ============================================================
// Pure-Rust core
// ============================================================

/// Compose the bytes that [`__torajs_str_print_err`] would write,
/// for unit-testability of the byte-slicing path. Production
/// callers use the extern wrapper which writes directly to stderr.
///
/// P11.1-S2.1 takes the Str's encoding so the composed buffer is
/// always a valid UTF-8 byte stream (which is what stderr / the
/// terminal expects). Latin-1 inputs with bytes > 0x7F expand to
/// 2-byte UTF-8 sequences; UTF-16 inputs are decoded + re-encoded
/// codepoint-by-codepoint.
#[inline]
pub fn format_print_err(payload: Option<(&[u8], bool)>) -> Vec<u8> {
    match payload {
        None => b"null\n".to_vec(),
        Some((bytes, is_latin1)) => {
            let mut out = encode_payload_utf8(bytes, is_latin1, 1);
            out.push(b'\n');
            out
        }
    }
}

/// Transcode a Str payload into UTF-8. Shared by
/// [`format_print_err`] (which appends a newline) and
/// [`format_write_err`] (which does not) — the transcoding is
/// identical, only the line terminator differs.
///
/// `extra_cap` pre-pays for bytes the caller appends afterwards so
/// the push can't reallocate. Worst-case payload capacity: every
/// Latin-1 supplement byte expands to 2 bytes; every UTF-16 code
/// unit expands up to 3 bytes (4 if it's a surrogate pair, but
/// those cover 2 code units so the per-unit bound stays 3).
#[inline]
fn encode_payload_utf8(bytes: &[u8], is_latin1: bool, extra_cap: usize) -> Vec<u8> {
    let cap = if is_latin1 {
        bytes.len() * 2 + extra_cap
    } else {
        (bytes.len() / 2) * 3 + extra_cap
    };
    let mut out = Vec::with_capacity(cap);
    if is_latin1 {
        for &b in bytes {
            write_utf8_for_latin1_byte(b, |u| out.push(u));
        }
    } else {
        iter_utf16_codepoints(bytes, |cp| {
            write_utf8_for_codepoint(cp, |u| out.push(u));
        });
    }
    out
}

/// Compose the bytes [`__torajs_str_write_err`] writes — the same
/// UTF-8 transcoding [`encode_payload_utf8`] performs, with **no**
/// trailing newline, and NULL rendering as nothing.
///
/// The two differences are what make this the right primitive for
/// composing a multi-part diagnostic line (an Error's `name`, then
/// `": "`, then its `message`): the parts must not each terminate
/// the line, and a NULL part of a partially-constructed value must
/// contribute nothing rather than the word `null`.
#[inline]
pub fn format_write_err(payload: Option<(&[u8], bool)>) -> Vec<u8> {
    match payload {
        None => Vec::new(),
        Some((bytes, is_latin1)) => encode_payload_utf8(bytes, is_latin1, 0),
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
}

/// Write a single byte to the shared stdout buffer. Thin wrapper
/// to keep the per-codepoint inline closure tight.
#[inline]
fn putc(b: u8) {
    unsafe {
        __torajs_io_putc_out(b as i32);
    }
}

/// `console.log(str)` — write `s`'s payload (UTF-8 transcoded from
/// the Str's native encoding) + newline to stdout via per-byte
/// putchar. NULL → `"null\n"`.
///
/// Uses putchar (NOT `std::io::stdout`) so the output shares C
/// stdio's stdout buffer with `print_i64` / `print_f64` /
/// `print_bool` — otherwise mixed-type `console.log` sequences
/// reorder on flush. See module docs for the cross-buffer detail.
///
/// # Safety
///
/// `s` must be either NULL or a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_print(s: *const u8) {
    if s.is_null() {
        for &b in b"null\n" {
            putc(b);
        }
        return;
    }
    let length = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() } as usize;
    if length > 0 {
        let is_latin1 = unsafe { header_is_latin1(s) };
        if is_latin1 {
            // Latin-1: 1 byte per code unit. ASCII subset writes
            // verbatim; bytes > 0x7F expand to a 2-byte UTF-8
            // sequence.
            let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), length) };
            for &b in bytes {
                write_utf8_for_latin1_byte(b, putc);
            }
        } else {
            // UTF-16: 2 bytes per code unit, payload = length * 2 bytes.
            let payload_bytes = length * 2;
            let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), payload_bytes) };
            iter_utf16_codepoints(bytes, |cp| {
                write_utf8_for_codepoint(cp, putc);
            });
        }
    }
    putc(b'\n');
}

/// `console.log(substr)` — write a Substr's view (parent bytes +
/// offset slice) + newline to stdout via per-byte putchar. Substr
/// layout `{ hdr@0, len@8, parent@16, offset@24 }` is read directly
/// (no materialize). NULL → `"null\n"`.
///
/// Same buffer-sharing concern as [`__torajs_str_print`]: this is
/// the console.log path for Substr-typed receivers, so it must use
/// the same stdio buffer as print_i64 / print_bool / str_print.
///
/// P11.1-S2.1 — the Substr's parent Str carries the encoding (a
/// Substr never independently changes encoding), so the parent
/// header drives the Latin-1 / UTF-16 dispatch here too. Substr's
/// own `len@8` is still a u64 byte count over the parent payload —
/// substr layout flip is queued for S5; until then the view stride
/// is `parent_encoding_bytes_per_unit` worth of payload starting at
/// `parent + STR_DATA_OFF + offset`.
///
/// # Safety
///
/// `v` must be NULL or a valid Substr heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_print(v: *const u8) {
    if v.is_null() {
        for &b in b"null\n" {
            putc(b);
        }
        return;
    }
    let cu_len = unsafe { (v.add(SUBSTR_LEN_OFF) as *const u64).read() } as usize;
    let parent = unsafe { (v.add(SUBSTR_PARENT_OFF) as *const *const u8).read() };
    let cu_offset = unsafe { (v.add(SUBSTR_OFFSET_OFF) as *const u64).read() } as usize;
    if cu_len > 0 {
        let is_latin1 = unsafe { header_is_latin1(parent) };
        let stride = if is_latin1 { 1 } else { 2 };
        let payload = unsafe {
            core::slice::from_raw_parts(
                parent.add(STR_DATA_OFF + cu_offset * stride),
                cu_len * stride,
            )
        };
        if is_latin1 {
            for &b in payload {
                write_utf8_for_latin1_byte(b, putc);
            }
        } else {
            iter_utf16_codepoints(payload, |cp| {
                write_utf8_for_codepoint(cp, putc);
            });
        }
    }
    putc(b'\n');
}

/// Write `s`'s payload (UTF-8 transcoded from the Str's native
/// encoding) + newline to stderr in one `write(2)`. NULL →
/// `"null\n"`.
///
/// Since RFC 20260812-console-sink knife 2 the `console.error(str)`
/// lowering no longer targets this symbol (it brackets the ordinary
/// `__torajs_str_print` with the io current-sink switch); the
/// remaining consumer is torajs-promise's unhandled-rejection
/// reporter, which calls it directly from runtime Rust for its
/// one-line reason rendering.
///
/// # Safety
///
/// `s` must be either NULL or a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_print_err(s: *const u8) {
    let payload = if s.is_null() {
        None
    } else {
        let length = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() } as usize;
        let is_latin1 = unsafe { header_is_latin1(s) };
        let payload_bytes = if is_latin1 { length } else { length * 2 };
        let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), payload_bytes) };
        Some((bytes, is_latin1))
    };
    crate::write_stderr(&format_print_err(payload));
}

/// Write `s`'s payload (UTF-8 transcoded from the Str's native
/// encoding) to stderr with **no** trailing newline. NULL → no-op.
///
/// A newline-free composing writer for
/// diagnostics that compose several Str parts into one line — the
/// unhandled-rejection reporter renders an Error as
/// `name` + `": "` + `message`, which `print_err` could not spell
/// because each part would terminate the line.
///
/// Transcoding matters here rather than a raw payload copy: a
/// Latin-1 `message` such as `"café"` stores `0xE9`, which is not
/// valid UTF-8 on its own and reaches the terminal as mojibake if
/// written verbatim.
///
/// # Safety
///
/// `s` must be null or point at a live Str block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_write_err(s: *const u8) {
    if s.is_null() {
        return;
    }
    let length = unsafe { (s.add(STR_LEN_OFF) as *const u32).read() } as usize;
    let is_latin1 = unsafe { header_is_latin1(s) };
    let payload_bytes = if is_latin1 { length } else { length * 2 };
    let bytes = unsafe { core::slice::from_raw_parts(s.add(STR_DATA_OFF), payload_bytes) };
    crate::write_stderr(&format_write_err(Some((bytes, is_latin1))));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_null_yields_literal_string() {
        assert_eq!(format_print_err(None), b"null\n");
    }

    #[test]
    fn format_empty_payload_yields_just_newline() {
        assert_eq!(format_print_err(Some((b"", true))), b"\n");
        assert_eq!(format_print_err(Some((b"", false))), b"\n");
    }

    #[test]
    fn write_err_null_yields_nothing() {
        assert!(format_write_err(None).is_empty());
    }

    #[test]
    fn write_err_omits_the_trailing_newline() {
        assert_eq!(format_write_err(Some((b"hello", true))), b"hello");
        assert!(format_write_err(Some((b"", true))).is_empty());
    }

    #[test]
    fn write_err_transcodes_like_print_err() {
        // 'é' = U+00E9 = Latin-1 byte 0xE9 → UTF-8 0xC3 0xA9. The
        // whole point of routing the reporter through this instead
        // of a raw payload copy.
        assert_eq!(
            format_write_err(Some((&[b'c', b'a', b'f', 0xE9], true))),
            b"caf\xC3\xA9"
        );
        // UTF-16 'A' (0x0041 LE) decodes to one ASCII byte.
        assert_eq!(format_write_err(Some((&[0x41, 0x00], false))), b"A");
    }

    #[test]
    fn latin1_ascii_writes_verbatim() {
        assert_eq!(format_write_err(Some((b"hello", true))), b"hello");
    }

    #[test]
    fn latin1_supplement_expands_to_2byte_utf8() {
        // 'é' = U+00E9 = Latin-1 byte 0xE9 → UTF-8 0xC3 0xA9
        assert_eq!(
            format_write_err(Some((&[b'c', b'a', b'f', 0xE9], true))),
            b"caf\xC3\xA9"
        );
    }

    #[test]
    fn utf16_bmp_decodes_to_utf8() {
        // U+4E2D ('中') little-endian → 0x2D 0x4E → UTF-8 0xE4 0xB8 0xAD
        assert_eq!(
            format_write_err(Some((&[0x2D, 0x4E], false))),
            b"\xE4\xB8\xAD"
        );
    }

    #[test]
    fn utf16_surrogate_pair_decodes_to_4byte_utf8() {
        // U+1F600 ('😀') surrogate pair: hi=0xD83D lo=0xDE00
        // little-endian → 0x3D 0xD8 0x00 0xDE
        // UTF-8 → 0xF0 0x9F 0x98 0x80
        assert_eq!(
            format_write_err(Some((&[0x3D, 0xD8, 0x00, 0xDE], false))),
            b"\xF0\x9F\x98\x80"
        );
    }

    #[test]
    fn utf16_mixed_ascii_and_surrogate_pair() {
        // 'A' (U+0041) + '😀' (U+1F600) → "\x41\x00\x3D\xD8\x00\xDE"
        // UTF-8 → 0x41 + 0xF0 0x9F 0x98 0x80
        let payload = &[0x41, 0x00, 0x3D, 0xD8, 0x00, 0xDE];
        assert_eq!(
            format_write_err(Some((payload, false))),
            b"A\xF0\x9F\x98\x80"
        );
    }

    #[test]
    fn utf16_lone_high_surrogate_yields_3byte_utf8() {
        // Lone high surrogate 0xD83D (no following low) — yielded
        // as numeric codepoint 0xD83D, which encodes to 3-byte
        // UTF-8 0xED 0xA0 0xBD per the formula. ES spec allows
        // this even though strictly not a valid Unicode scalar.
        assert_eq!(
            format_write_err(Some((&[0x3D, 0xD8], false))),
            b"\xED\xA0\xBD"
        );
    }
}
