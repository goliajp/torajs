//! `extern "C"` Str allocation entry points — what toolchain-emitted
//! code and the still-C-shaped ABI call to get a Str cell.
//!
//! Pulled out of [`crate::block`] to keep that file under the
//! 500-prod-LOC file-size hard limit (`rules/common/file-size.md`),
//! along the seam that file already drew for itself: `block.rs`
//! answers what a Str cell *is* and how it is taken and given back,
//! this one answers what a caller across the ABI boundary asks for.
//! `str_drop.rs` is the same split on the release side. Pure
//! mechanical pull, no semantic change.

use crate::block::StrBlock;

/// Pool-aware Str allocation. Mirrors the pre-rewrite C
/// `__torajs_str_alloc_pooled(uint64_t len) -> uint8_t *`. The
/// toolchain-emitted `__torajs_str_alloc` delegates to this for
/// short strings.
///
/// Returns a fresh refcount=1 block with `len` payload bytes
/// reserved (uninitialized). On allocator failure the function
/// panics — matching the pre-rewrite "abort on OOM" behavior
/// (`malloc` returning null leads to `expect` here; rc_inc /
/// rc_dec semantics aren't reached).
///
/// P11.1-S1: FFI ABI keeps `len: u64` for compatibility with the
/// IR-emitted call sites; internally truncated to `u32` since
/// post-S1 `length` lives in a u32 field. Encoding hard-coded to
/// Latin-1 — every existing caller built on byte-Str semantics
/// (`len` = byte count) maps trivially to Latin-1 (`length` =
/// code unit = byte for Latin-1 payloads).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(len: u64) -> *mut u8 {
    StrBlock::alloc(len as u32).into_raw()
}

/// Encoding-aware sibling of [`__torajs_str_alloc_pooled`] (RFC
/// 20260711 `arr.join` follow-up). `len` counts CODE UNITS;
/// `is_latin1 != 0` selects the 1-byte payload stride, zero selects
/// UTF-16 LE (2 bytes per unit). Cross-staticlib consumers
/// (torajs-arr's join kernels) that fold a widest-of-inputs output
/// encoding need to allocate the wide layout directly — the legacy
/// entry above hard-codes Latin-1.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled_enc(len: u64, is_latin1: i64) -> *mut u8 {
    StrBlock::alloc_with_encoding(len as u32, is_latin1 != 0).into_raw()
}

/// ASCII-certain variant of [`__torajs_str_alloc`] — Round 5 attack
/// str-replace #5 (2026-07-03). The caller has already established
/// every byte of `src[0..len]` is ≤ 0x7F (e.g. the regex replace
/// builder whose haystack AND replacement both passed the
/// `str_slice_ascii_view` scan), so the per-char classification
/// scan inside `__torajs_str_alloc` is provably redundant: alloc
/// the Latin-1 layout and memcpy verbatim.
///
/// # Safety
///
/// `src` must point at `len` readable bytes, ALL ≤ 0x7F (or be NULL
/// when `len == 0`). Returned pointer is a fresh refcount=1 Str
/// block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_ascii(src: *const u8, len: i64) -> *mut u8 {
    let len_u = len as usize;
    let length = len_u as u32;
    let mut block = StrBlock::alloc_with_encoding(length, true);
    if len_u > 0 {
        let src_slice = unsafe { core::slice::from_raw_parts(src, len_u) };
        let dst = unsafe { block.as_bytes_mut(length) };
        dst.copy_from_slice(src_slice);
    }
    block.into_raw()
}

/// Str alloc + UTF-8 → canonical encoding payload write in one call.
///
/// Pre-S2 this was a plain `alloc + memcpy` (input bytes copied
/// verbatim). P11.1-S2.1 promoted the contract: `src[0..len]` is a
/// well-formed UTF-8 byte stream; the helper scans it once to
/// decide the canonical encoding (Latin-1 if every codepoint
/// ≤ 0xFF, else UTF-16 LE with surrogate pair encoding for
/// supplementary planes), allocates the matching layout via
/// [`StrBlock::alloc_with_encoding`], and writes the re-encoded
/// payload. This canonicalises the runtime side of build-time
/// `StringLiteral::encode_from_str` — every Str block ever
/// observed by the print / concat / eq / search ops is encoded
/// consistently, and same-content / same-encoding Strs compare
/// equal byte-for-byte without an explicit normalisation pass.
///
/// Used by the materialise paths in `torajs-anyvalue`
/// (`materialize_short_str` packs UTF-8 bytes from the NaN-box
/// payload into a Heap+Str so the `(tag, value)` pair-API
/// downstream sees a Tag::Str pointer) and any other helper that
/// builds a Str from a UTF-8 byte buffer at runtime. Sites that
/// already hold an encoded payload (concat / case-fold / etc) go
/// directly through [`StrBlock::alloc_with_encoding`] instead.
///
/// # Safety
///
/// `src` must point at a readable region of at least `len` bytes
/// (or be NULL when `len == 0`). The bytes must form a well-
/// formed UTF-8 sequence — non-UTF-8 inputs silently get the
/// fallback Latin-1 path (every byte ≤ 0x7F is ASCII so it stays
/// safe; bytes 0x80-0xFF would be misclassified as Latin-1
/// codepoints if mixed with stray UTF-8 continuation bytes, but
/// no current caller hands such garbage). Returned pointer is a
/// fresh refcount=1 Str block owned by the caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8 {
    let len_u = len as usize;
    if len_u == 0 {
        let block = StrBlock::alloc_with_encoding(0, true);
        return block.into_raw();
    }
    // SAFETY: caller guarantees `src..src+len` is readable and
    // well-formed UTF-8.
    let src_slice = unsafe { core::slice::from_raw_parts(src, len_u) };
    let utf8 = unsafe { core::str::from_utf8_unchecked(src_slice) };
    // An all-ASCII payload copies verbatim into the Latin-1 layout,
    // every byte already matching its own codepoint — the shape the
    // pre-S2 `alloc + memcpy` had. And "all ASCII" is a question
    // about bytes, not characters: a well-formed UTF-8 sequence is
    // non-ASCII exactly when some byte has its high bit set.
    //
    // Asking it that way is a flat byte scan. Decoding the source
    // into `char`s to take a maximum codepoint answered the same
    // question one branchy step at a time, and every caller handing
    // over an ASCII buffer — which is nearly all of them — paid that
    // walk in full before reaching this copy.
    if !src_slice.iter().any(|b| *b >= 0x80) {
        let length = len_u as u32;
        let mut block = StrBlock::alloc_with_encoding(length, true);
        let dst = unsafe { block.as_bytes_mut(length) };
        dst.copy_from_slice(src_slice);
        return block.into_raw();
    }
    // Something above ASCII is in there, so the widest codepoint now
    // has to be found to choose between the one-byte Latin-1 layout
    // and UTF-16, and that does mean decoding.
    let mut max_cp: u32 = 0;
    for c in utf8.chars() {
        let cp = c as u32;
        if cp > max_cp {
            max_cp = cp;
        }
    }
    if max_cp <= 0xFF {
        // Latin-1 supplement (0x80..=0xFF). Re-encode codepoint-
        // by-codepoint into one byte each.
        let length = utf8.chars().count() as u32;
        let mut block = StrBlock::alloc_with_encoding(length, true);
        let dst = unsafe { block.as_bytes_mut(length) };
        for (i, c) in utf8.chars().enumerate() {
            dst[i] = c as u8;
        }
        return block.into_raw();
    }
    // UTF-16 LE — BMP codepoints get a single u16; supplementary
    // plane gets a surrogate pair. Walk twice: first to count code
    // units for the length field, then to fill the payload.
    let mut length: u32 = 0;
    for c in utf8.chars() {
        length += if (c as u32) > 0xFFFF { 2 } else { 1 };
    }
    let byte_cap = (length as usize) * 2;
    let mut block = StrBlock::alloc_with_encoding(length, false);
    let dst = unsafe { block.as_bytes_mut(byte_cap as u32) };
    let mut i = 0usize;
    for c in utf8.chars() {
        let cp = c as u32;
        if cp <= 0xFFFF {
            let u = cp as u16;
            let le = u.to_le_bytes();
            dst[i] = le[0];
            dst[i + 1] = le[1];
            i += 2;
        } else {
            let cp_off = cp - 0x10000;
            let hi = (0xD800 | (cp_off >> 10)) as u16;
            let lo = (0xDC00 | (cp_off & 0x3FF)) as u16;
            let hi_le = hi.to_le_bytes();
            let lo_le = lo.to_le_bytes();
            dst[i] = hi_le[0];
            dst[i + 1] = hi_le[1];
            dst[i + 2] = lo_le[0];
            dst[i + 3] = lo_le[1];
            i += 4;
        }
    }
    block.into_raw()
}
