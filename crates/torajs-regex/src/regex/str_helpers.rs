//! Str-payload transcoding + fresh-Str allocation helpers extracted
//! from [`super`] as the rotation-196 file-size sweep — the parent
//! `regex/mod.rs` had drifted to 538 LOC over the strictness wave
//! (registered in `rules/torajs-file-size-debt.md` rotation 185
//! audit). Zero-copy ASCII view + full transcode + two allocator
//! shells share the same STR_HDR_SIZE / `__torajs_str_alloc*` extern
//! surface, so they cluster naturally into a sibling module.
//!
//! `super::mod` re-exports every symbol at the same paths so the ~7
//! callers under `regex/*.rs` (`match_op` / `test_find` /
//! `match_indices` / `match_all` / `replace*` / `split` /
//! `static_keys`) keep their `use super::{haystack, ...}` imports
//! byte-for-byte unchanged.

use core::ffi::c_void;

use alloc::vec::Vec;

use super::{__torajs_rc_inc, __torajs_str_alloc, __torajs_str_alloc_ascii, STR_HDR_SIZE};

/// View a tora `Str *` as a `&[u8]` of its payload. Safety: `p`
/// must point at a live Str whose header is well-formed and whose
/// payload remains valid for the borrow's lifetime.
///
/// # Safety
///
/// Caller guarantees that `p` is non-null, well-aligned, and
/// references a tora-Str-layout block whose bytes outlive `'a`.
/// chunk 7.7 v2 step 12 C2 Phase B-1 attack #A — zero-copy `&[u8]`
/// view over a tora `Str *` payload when (and only when) the payload
/// is ASCII Latin-1. Returns `None` for non-ASCII Latin-1 / UTF-16
/// payloads — caller must fall back to [`haystack`] for those
/// (transcode allocates a fresh `Vec<u8>`).
///
/// The ASCII Latin-1 fast path is the overwhelmingly common case for
/// regex match bench fixtures (and any `match` / `exec` against a
/// human-keyboard string), so taking a borrow there avoids ~80 ns/iter
/// of `payload.to_vec()` alloc+memcpy on `__torajs_str_match_regex` /
/// `__torajs_regex_exec` hot-path call sites.
///
/// # Safety
///
/// Same contract as [`str_units`] — `p` is non-null, well-aligned,
/// references a live tora-Str-layout block. Additionally the caller
/// must ensure the returned slice's lifetime `'a` does not outlive
/// the underlying Str buffer; in practice the slice is bound to the
/// `__torajs_str_match_regex` / `__torajs_regex_exec` call's stack
/// frame, which is shorter than any caller-held Str reference.
pub unsafe fn str_slice_ascii_view<'a>(p: *const c_void) -> Option<&'a [u8]> {
    let s = p as *const u8;
    let length = unsafe { *(s.add(8) as *const u32) };
    let flags = unsafe { *(s.add(6) as *const u16) };
    let is_latin1 = (flags & 0x0002) != 0;
    if !is_latin1 {
        return None;
    }
    let payload =
        unsafe { core::slice::from_raw_parts::<'a, u8>(s.add(STR_HDR_SIZE), length as usize) };
    // `<[u8]>::is_ascii` scans a machine word at a time; the byte
    // loop it replaces was 18% of `/a.+c/s` over a 27-byte haystack
    // (rotation 667 leaf profile: four instructions per byte, every
    // call, ahead of the match itself).
    if !payload.is_ascii() {
        return None;
    }
    Some(payload)
}

/// The Str transcoded as a sequence of UTF-16 CODE UNITS: every unit
/// on its own, a surrogate pair as two three-byte forms. This is the
/// form a pattern without `u` / `v` matches over (§22.2.2.1), the
/// canonical `src_bytes` of every RegExp, and the round-trip form
/// for strings the engine only copies.
///
/// # Safety
/// `p` is a live tora Str pointer.
pub unsafe fn str_units(p: *const c_void) -> Vec<u8> {
    unsafe { str_slice(p, false) }
}

/// The Str transcoded as a sequence of CODE POINTS: a surrogate pair
/// becomes its supplementary code point (four bytes), a lone
/// surrogate stays a three-byte form (WTF-8). This is the form
/// `u` / `v` mode matches over, and what `RegExp.escape` walks.
///
/// # Safety
/// `p` is a live tora Str pointer.
pub unsafe fn str_code_points(p: *const c_void) -> Vec<u8> {
    unsafe { str_slice(p, true) }
}

/// The haystack for `re`: code points under `u` / `v`, code units
/// otherwise — the same form the pattern was compiled over.
///
/// # Safety
/// `p` is a live tora Str pointer.
pub unsafe fn haystack(re: &super::RegExp, p: *const c_void) -> Vec<u8> {
    unsafe { str_slice(p, crate::flags::unicode_mode(re.flags)) }
}

unsafe fn str_slice(p: *const c_void, merge_pairs: bool) -> Vec<u8> {
    // P11.1-S2.1 — Str payload is encoded (Latin-1 or UTF-16 LE)
    // rather than raw UTF-8 bytes. The regex engine operates on
    // UTF-8 byte streams, so haystacks / patterns transcode here
    // before reaching the matching code. `merge_pairs` picks the
    // code-point form (surrogate pairs joined) over the code-unit
    // form — see the two named entries above. Returns an owned Vec so
    // call sites uniformly hold the buffer for the duration of
    // the match; ASCII-only Latin-1 payloads still allocate +
    // `to_vec` once each match, but the regex hot loops dominate
    // that cost so the simplicity wins. (A `Cow` variant was
    // explored but every downstream consumer ends up owning the
    // buffer anyway via VM iteration / replace builders.)
    let s = p as *const u8;
    let length = unsafe { *(s.add(8) as *const u32) };
    let flags = unsafe { *(s.add(6) as *const u16) };
    let is_latin1 = (flags & 0x0002) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(s.add(STR_HDR_SIZE), byte_cnt) };
    if is_latin1 && payload.is_ascii() {
        return payload.to_vec();
    }
    if is_latin1 {
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
    let mut out = Vec::with_capacity((length as usize) * 3);
    let mut i = 0usize;
    while i + 1 < payload.len() {
        let cu = u16::from_le_bytes([payload[i], payload[i + 1]]) as u32;
        let cp = if merge_pairs && (0xD800..=0xDBFF).contains(&cu) && i + 3 < payload.len() {
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

/// Allocate a fresh refcounted `Str` of `data.len()` bytes via the
/// small-Str pool path; copy `data` into the payload. Returns the
/// pool-aligned Str pointer (rc=1).
///
/// # Safety
///
/// Calls into the C `__torajs_str_alloc_pooled` allocator (link-
/// time). The returned pointer must be released via
/// `__torajs_str_drop`.
pub unsafe fn str_from_bytes(data: &[u8]) -> *mut u8 {
    // P11.1-S2.1 — route through the canonical-encoding alloc so
    // returned match-fragment Strs carry the correct encoding flag
    // and downstream print / concat see them with consistent
    // semantics. Input `data` is a UTF-8 byte slice (either the
    // already-transcoded haystack returned by `haystack`, or a
    // replacement-builder buffer that the regex engine assembled
    // codepoint-by-codepoint).
    unsafe { __torajs_str_alloc(data.as_ptr(), data.len() as i64) }
}

/// ASCII-certain sibling of [`str_from_bytes`] — Round 5 attack
/// str-replace #5. Caller proves every byte of `data` is ≤ 0x7F
/// (haystack and replacement both passed `str_slice_ascii_view`),
/// skipping the encoding-classification scan in
/// `__torajs_str_alloc`.
///
/// # Safety
///
/// Same allocator contract as [`str_from_bytes`]; additionally all
/// bytes of `data` must be ASCII.
pub unsafe fn str_from_bytes_ascii(data: &[u8]) -> *mut u8 {
    unsafe { __torajs_str_alloc_ascii(data.as_ptr(), data.len() as i64) }
}

/// The string for the byte span `start..end` of a match over
/// `subject`, whose searched bytes are `s`.
///
/// When `s` is the subject's own ASCII payload
/// (`subject_is_ascii_view`) and the span is all of it, the answer is
/// the subject itself with one more share: strings are immutable and
/// carry no identity, so a whole-string hit (`/^\d+$/.exec(id)`, any
/// validating pattern) needs no second copy of the text. The extra
/// share also keeps `__torajs_str_append` off its in-place branch,
/// which requires refcount 1. Any other ASCII span skips the encoding
/// scan the view already paid for.
///
/// # Safety
///
/// `subject` is a live tora Str pointer, `0 <= start <= end <= s.len()`,
/// and `subject_is_ascii_view` is true only when `s` came from
/// [`str_slice_ascii_view`] on `subject`.
pub unsafe fn span_str(
    subject: *const c_void,
    s: &[u8],
    start: i64,
    end: i64,
    subject_is_ascii_view: bool,
) -> *mut u8 {
    let span = &s[start as usize..end as usize];
    if !subject_is_ascii_view {
        return unsafe { str_from_bytes(span) };
    }
    if span.len() == s.len() {
        // A `.rodata` literal keeps no count (the share is free), so
        // the call into torajs-rc is skipped for it; flags u16 @6.
        let flags = unsafe { *((subject as *const u8).add(6) as *const u16) };
        if flags & super::static_keys::FLAG_STATIC_LITERAL == 0 {
            unsafe { __torajs_rc_inc(subject as *mut c_void) };
        }
        return subject as *mut u8;
    }
    unsafe { str_from_bytes_ascii(span) }
}
