//! `__torajs_str_split` + `__torajs_split_iter_init` / `_drop`.
//!
//! `__torajs_str_split` builds the single-block Arr-with-inline-
//! substrs layout described in [`crate::split`]'s module docs.
//! The pool fast-path lives in [`crate::split::pool`], the token
//! count for the general build in [`crate::split::count`], and the
//! single-pass lane for a Latin-1 string cut on one byte in
//! [`crate::split::byte_sep`]; this file holds the entry, the cell /
//! header writers and the general two-pass fill. The iterator
//! surface is in [`crate::split::iter`].
//!
//! Bit-for-bit parity with the pre-rewrite C
//! `__torajs_str_split` is required — the SPLIT_BLOCK + inline-
//! substr layout interacts with the IR-emitted `__torajs_arr_*`
//! free dispatch + the Substr drop chain in ways that any layout
//! drift would break silently.

use core::ffi::c_void;
use core::ptr::NonNull;

use torajs_rc::{__torajs_rc_inc, FLAG_SPLIT_BLOCK, FLAG_STATIC_LITERAL, HeapHeader, Tag};

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::split::byte_sep;
use crate::split::count::out_count;
use crate::split::pool::{self, ARR_HDR_SIZE};
use crate::substr::{
    FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF, SUBSTR_SIZE,
};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
pub(crate) unsafe fn str_len(p: *const u8) -> u32 {
    unsafe { (p.add(STR_LEN_OFF) as *const u32).read() }
}

/// Read a Str's `(payload_bytes, code_unit_count, is_latin1)` view.
/// Used by the split entry to size scans + decide stride.
#[inline]
unsafe fn str_view<'a>(p: *const u8) -> (&'a [u8], u32, bool) {
    let length = unsafe { (p.add(STR_LEN_OFF) as *const u32).read() };
    let header = unsafe { &*(p as *const HeapHeader) };
    let is_latin1 = (header.flags & STR_FLAG_IS_LATIN1) != 0;
    let byte_cnt = if is_latin1 {
        length as usize
    } else {
        (length as usize) * 2
    };
    let payload = unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), byte_cnt) };
    (payload, length, is_latin1)
}

/// Widen a Latin-1 byte payload to UTF-16 LE — each input byte
/// becomes a `(byte, 0)` u16 pair. Used when the haystack is
/// UTF-16 but the separator is Latin-1.
fn widen_latin1_to_utf16(src: &[u8]) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(src.len() * 2);
    for &b in src {
        out.push(b);
        out.push(0);
    }
    out
}

// ============================================================
// Inline substr writer
// ============================================================

/// Initialize one inline substr struct at `substr_slot` and store
/// its address into `*arr_ptr_slot`. Bumps `parent`'s refcount when
/// `PARENT_RC` is true; const-generic so the static-parent fast path
/// (caller observed `FLAG_STATIC_LITERAL` on `parent`) monomorphizes
/// to zero `rc_inc` work — rc_inc on a static literal is a no-op by
/// design, so eliding it preserves the rc balance with the matching
/// `drop_pool_aware` substr drop (whose rc_dec on parent also early-
/// returns for static literals).
///
/// Mirrors the C `__torajs_split_init_inline` bit-for-bit. The
/// header carries `Tag::Str` (not `Tag::Substr`) + `FLAG_SUBSTR_INLINE`
/// — that's how the C runtime distinguishes "inline view sharing
/// the enclosing arr block's allocation" from a standalone Substr
/// alloc'd via [`crate::substr::__torajs_substr_create`].
///
/// # Safety
///
/// `substr_slot` must be a 32-byte writable region; `arr_ptr_slot`
/// must be a writable `*mut u8` slot; `parent` must be a valid
/// Str heap pointer (the rc_inc call dereferences its header when
/// `PARENT_RC`).
#[inline]
pub(super) unsafe fn split_init_inline<const PARENT_RC: bool>(
    substr_slot: *mut u8,
    arr_ptr_slot: *mut *mut u8,
    parent: *const u8,
    // Substr offset / len are CODE-UNIT values (P11.1-S5); byte
    // positions recover through the parent's encoding stride.
    offset: u64,
    len: u64,
) {
    let header = HeapHeader {
        refcount: 1,
        type_tag: Tag::Str as u16,
        // FLAG_SUBSTR_INLINE keeps the existing drop-path dispatch
        // (inline drop skips own-rc dec); FLAG_SUBSTR_VIEW lets the
        // anyvalue print dispatch route to substr_print instead of
        // the Str print walker (whose +16 inline-bytes read would
        // garble the substr's parent-ptr@+16 / offset@+24 fields).
        flags: FLAG_SUBSTR_INLINE | crate::substr::FLAG_SUBSTR_VIEW,
    };
    unsafe {
        (substr_slot as *mut HeapHeader).write(header);
        (substr_slot.add(SUBSTR_LEN_OFF) as *mut u64).write(len);
        (substr_slot.add(SUBSTR_PARENT_OFF) as *mut *const u8).write(parent);
        (substr_slot.add(SUBSTR_OFFSET_OFF) as *mut u64).write(offset);
        if PARENT_RC {
            __torajs_rc_inc(parent as *mut c_void);
        }
        arr_ptr_slot.write(substr_slot);
    }
}

// ============================================================
// Arr header writer
// ============================================================

/// Initialize the Arr header on a fresh split block: refcount=1,
/// tag=Arr, flags=SPLIT_BLOCK, len/cap = `out_count`, head=0, and
/// (Round 4 chunk 5a) props_dynobj = NULL at offset 24.
///
/// # Safety
///
/// `block` must point at a writable region ≥ `ARR_HDR_SIZE`.
#[inline]
pub(super) unsafe fn write_arr_header(block: NonNull<u8>, out_count: u64) {
    let header = HeapHeader {
        refcount: 1,
        type_tag: Tag::Arr as u16,
        flags: FLAG_SPLIT_BLOCK,
    };
    unsafe {
        (block.as_ptr() as *mut HeapHeader).write(header);
        (block.as_ptr().add(8) as *mut u64).write(out_count);
        // cap + head share a u64 slot at +16 (cap = low u32, head =
        // high u32). cap = out_count, head = 0 — write as a single
        // u64 store to mirror the C macro pair.
        (block.as_ptr().add(16) as *mut u64).write(out_count & 0xFFFF_FFFF);
        // Round 4 chunk 5a — inline props_dynobj slot initialized to
        // NULL. Split blocks rarely carry `arr.x = v` writes, but the
        // slot must be valid for the eventual inline arrprops dispatch
        // (chunk 5b+) to read a clean pointer.
        (block.as_ptr().add(24) as *mut u64).write(0);
        // B1 — data pointer, permanently self-referential (split
        // blocks never grow).
        (block.as_ptr().add(32) as *mut *mut u8).write(block.as_ptr().add(ARR_HDR_SIZE));
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// Build the 1-element "no match" block: an Arr containing a
/// single inline Substr that aliases the whole of `s`
/// (offset=0, length=`len_cu` code units). Reused by:
///  - `__torajs_str_split` Latin-1-haystack / UTF-16-needle path
///    (match is structurally impossible — emit `[s]` directly)
///  - `__torajs_str_split_no_sep` (separator is `undefined` per
///    ES §22.1.3.21 step 4 — return `[S]`)
///
/// # Safety
///
/// `s` must be a valid Str heap block; `len_cu` its code-unit
/// count.
#[inline]
unsafe fn single_token_block<const PARENT_RC: bool>(s: *const u8, len_cu: u64) -> *mut u8 {
    let block = pool::alloc(1);
    unsafe { write_arr_header(block, 1) };
    let slots_size = 8usize;
    let substrs_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE + slots_size) };
    let slots_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };
    unsafe {
        split_init_inline::<PARENT_RC>(substrs_base, slots_base, s, 0, len_cu);
    }
    block.as_ptr()
}

/// True iff `s`'s `HeapHeader::flags` has `FLAG_STATIC_LITERAL` set
/// — `.rodata`-baked Str literals whose rc operations are no-ops by
/// the rc-runtime contract. Caller hoists this check out of the
/// per-substr inline-init loop so the per-substr `__torajs_rc_inc`
/// call site can be eliminated entirely on the static-parent path.
///
/// # Safety
/// `s` must point at a valid Str heap block (header at offset 0).
#[inline]
unsafe fn parent_is_static_literal(s: *const u8) -> bool {
    let header = unsafe { &*(s as *const HeapHeader) };
    header.flags & FLAG_STATIC_LITERAL != 0
}

/// Inner fill loop: emits all inline substrs into the pre-allocated
/// split block. Const-generic over `PARENT_RC` so the caller-hoisted
/// `FLAG_STATIC_LITERAL` check fully monomorphizes both arms — the
/// static-parent monomorphization has zero `__torajs_rc_inc` call
/// sites in the per-substr inner body.
///
/// # Safety
/// `substrs_base` / `slots_base` are the pre-computed bases inside a
/// fresh `pool::alloc(out_count)` block; `s` is the parent Str heap
/// ptr; encoded slice/cu/stride/sep_bytes pre-validated by the caller.
#[inline(always)]
unsafe fn fill_substrs<const PARENT_RC: bool>(
    s: *const u8,
    s_payload: &[u8],
    s_len_cu: u32,
    sep_bytes: &[u8],
    sep_len_cu: u32,
    stride: usize,
    substrs_base: *mut u8,
    slots_base: *mut *mut u8,
) {
    if sep_len_cu == 0 {
        // Per-char split — emit one Substr per code unit of `s`.
        for k in 0..(s_len_cu as usize) {
            unsafe {
                split_init_inline::<PARENT_RC>(
                    substrs_base.add(k * SUBSTR_SIZE),
                    slots_base.add(k),
                    s,
                    k as u64,
                    1,
                );
            }
        }
        return;
    }

    let sep_byte_len = sep_bytes.len();
    let s_byte_len = s_payload.len();
    let mut ix: usize = 0;
    let mut start_byte: usize = 0;
    if stride == 1 && sep_byte_len == 1 && sep_byte_len <= s_byte_len {
        // V0.2 P14-S6 — single-byte-needle SIMD fast path
        // (mirror of `count_matches`'s fast arm). LLVM
        // auto-vectorizes the byte-equality scan to NEON
        // `pcmpeq + popcount` style code on ARM64.
        let target = sep_bytes[0];
        let mut i = 0;
        while i < s_byte_len {
            if s_payload[i] == target {
                unsafe {
                    split_init_inline::<PARENT_RC>(
                        substrs_base.add(ix * SUBSTR_SIZE),
                        slots_base.add(ix),
                        s,
                        start_byte as u64,
                        (i - start_byte) as u64,
                    );
                }
                ix += 1;
                i += 1;
                start_byte = i;
            } else {
                i += 1;
            }
        }
    } else if sep_byte_len <= s_byte_len {
        let limit = s_byte_len - sep_byte_len;
        let mut i: usize = 0;
        while i <= limit {
            if &s_payload[i..i + sep_byte_len] == sep_bytes {
                unsafe {
                    split_init_inline::<PARENT_RC>(
                        substrs_base.add(ix * SUBSTR_SIZE),
                        slots_base.add(ix),
                        s,
                        (start_byte / stride) as u64,
                        ((i - start_byte) / stride) as u64,
                    );
                }
                ix += 1;
                i += sep_byte_len;
                start_byte = i;
            } else {
                i += stride;
            }
        }
    }
    // Trailing token (may be empty if s ends with sep).
    unsafe {
        split_init_inline::<PARENT_RC>(
            substrs_base.add(ix * SUBSTR_SIZE),
            slots_base.add(ix),
            s,
            (start_byte / stride) as u64,
            ((s_byte_len - start_byte) / stride) as u64,
        );
    }
}

/// `s.split(sep)` — fresh `string[]` of substrings split by `sep`.
/// Returns a single block carrying:
/// - Arr header (24 bytes) with `FLAG_SPLIT_BLOCK`
/// - N ptr slots (8 bytes each, N = `out_count`)
/// - N inline 32-byte substr structs (FLAG_SUBSTR_INLINE)
///
/// Each slot's ptr points at its corresponding inline substr.
/// Empty `sep` splits per-char ("ab".split("") → ["a","b"]).
/// Per-iter malloc count: 1.
///
/// # Safety
///
/// Both `s` and `sep` must be valid Str heap blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split(s: *const u8, sep: *const u8) -> *mut u8 {
    // V0.2 P14-S9 — hoist `FLAG_STATIC_LITERAL` check on `parent` once;
    // the inline-substr fill loop monomorphizes via `PARENT_RC` so the
    // dominant `.rodata`-string bench shape (`"...".split(" ")` in a
    // tight loop) drops every per-substr `__torajs_rc_inc(parent)` FFI
    // call. The matching `drop_pool_aware` substr drop's `rc_dec` on
    // parent also early-returns for static literals, so balance holds.
    let parent_static = unsafe { parent_is_static_literal(s) };
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    let (sep_payload, _, sep_latin1) = unsafe { str_view(sep) };
    // The shape nearly every split in the corpus has — a Latin-1
    // string cut on one byte — takes the single-pass lane; everything
    // else (UTF-16 on either side, multi-byte or empty separators)
    // goes through the general two-pass build, kept out of line so
    // this entry keeps a small frame.
    if s_latin1 && sep_latin1 && sep_payload.len() == 1 && !s_payload.is_empty() {
        let target = sep_payload[0];
        if parent_static {
            if let Some(block) = unsafe { byte_sep::split_byte_sep::<false>(s, s_payload, target) }
            {
                return block;
            }
        } else if let Some(block) =
            unsafe { byte_sep::split_byte_sep::<true>(s, s_payload, target) }
        {
            return block;
        }
    }
    unsafe { split_general(s, sep, parent_static) }
}

/// The general two-pass build: `out_count` sizes the block, then
/// `fill_substrs` emits the cells. Every encoding / separator shape
/// is handled here; the hot Latin-1 byte-separator shape takes
/// [`split_byte_sep`] first.
///
/// # Safety
///
/// Both `s` and `sep` must be valid Str heap blocks.
#[cold]
#[inline(never)]
unsafe fn split_general(s: *const u8, sep: *const u8, parent_static: bool) -> *mut u8 {
    let (s_payload, s_len_cu, s_latin1) = unsafe { str_view(s) };
    let (sep_payload, sep_len_cu, sep_latin1) = unsafe { str_view(sep) };
    let stride: usize = if s_latin1 { 1 } else { 2 };

    // Canonical-encoding short-circuit + needle widening, mirroring
    // `lookup.rs::align_haystack_needle`:
    // - Latin-1 haystack + UTF-16 needle → no match possible (the
    //   needle's codepoint > 0xFF can't occur in a Latin-1
    //   payload). Result is a single-token array containing all of
    //   `s`.
    // - UTF-16 haystack + Latin-1 needle → widen the needle to
    //   UTF-16 LE so byte-aligned scanning matches the haystack's
    //   u16 grid.
    let widened_owned;
    let sep_bytes: &[u8] = match (s_latin1, sep_latin1) {
        (true, false) => {
            // Impossible match — emit a single trailing token
            // covering the whole haystack and return.
            return if parent_static {
                unsafe { single_token_block::<false>(s, s_len_cu as u64) }
            } else {
                unsafe { single_token_block::<true>(s, s_len_cu as u64) }
            };
        }
        (false, true) => {
            widened_owned = widen_latin1_to_utf16(sep_payload);
            widened_owned.as_slice()
        }
        _ => sep_payload,
    };

    let oc = out_count(s_payload, sep_bytes, stride);
    let block = pool::alloc(oc);
    unsafe { write_arr_header(block, oc) };

    let slots_size = (oc as usize) * 8;
    let substrs_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE + slots_size) };
    let slots_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };

    if parent_static {
        unsafe {
            fill_substrs::<false>(
                s,
                s_payload,
                s_len_cu,
                sep_bytes,
                sep_len_cu,
                stride,
                substrs_base,
                slots_base,
            );
        }
    } else {
        unsafe {
            fill_substrs::<true>(
                s,
                s_payload,
                s_len_cu,
                sep_bytes,
                sep_len_cu,
                stride,
                substrs_base,
                slots_base,
            );
        }
    }
    block.as_ptr()
}

/// `s.split()` (no separator argument) per ES §22.1.3.21 step 4:
/// separator is `undefined` → return a fresh `Array<Substr>` with
/// one element, the full string `s` as a Substr view.
///
/// Equivalent compile-time shape to `__torajs_str_split(s, sep)` so
/// downstream Substr-aware method dispatch routes uniformly. The
/// ssa-lower side routes here when the user wrote `expr.split()`
/// with no argument; previously the lower emitted a 1-arg call to
/// `__torajs_str_split(s, sep)` whose missing `sep` slot read whatever
/// register garbage the call site happened to leave — single-call
/// programs survived (registers held a coincidentally-walkable
/// pointer), but any prior `.split(arg)` call shifted the residual
/// register state and the next no-arg call SIGSEGV'd inside the
/// sep `str_view`.
///
/// # Safety
///
/// `s` must be a valid Str heap block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_split_no_sep(s: *const u8) -> *mut u8 {
    let parent_static = unsafe { parent_is_static_literal(s) };
    let (_, s_len_cu, _) = unsafe { str_view(s) };
    if parent_static {
        unsafe { single_token_block::<false>(s, s_len_cu as u64) }
    } else {
        unsafe { single_token_block::<true>(s, s_len_cu as u64) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{__torajs_str_free, StrBlock};
    use crate::split::iter::{
        __torajs_split_iter_drop, __torajs_split_iter_init, __torajs_split_iter_next, SplitIter,
    };
    use alloc::{vec, vec::Vec};

    // ARR layout consts (mirror C runtime_str.c — cross-layer until
    // torajs-arr crate lands). Only the offsets the test path reads
    // are declared here; ARR_CAP_OFF lives in split::pool which the
    // production code uses for the free dispatch.
    const ARR_LEN_OFF: usize = 8;
    const ARR_DATA_OFF: usize = ARR_HDR_SIZE;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u32);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
        dst.copy_from_slice(payload);
        b.into_raw()
    }

    /// Reach into a split block and pull each token's bytes out
    /// for assertion. Uses the inline substr layout (SUBSTR_PARENT
    /// → STR_DATA, SUBSTR_OFFSET, SUBSTR_LEN). Substr offset/len are
    /// code-unit values — byte positions recover through the
    /// parent's encoding stride.
    unsafe fn read_split_tokens(block: *mut u8) -> Vec<Vec<u8>> {
        let len = unsafe { (block.add(ARR_LEN_OFF) as *const u64).read() } as usize;
        let slots = unsafe { block.add(ARR_DATA_OFF) as *const *mut u8 };
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let substr = unsafe { *slots.add(i) };
            let units = unsafe { (substr.add(SUBSTR_LEN_OFF) as *const u64).read() } as usize;
            let parent = unsafe { *(substr.add(SUBSTR_PARENT_OFF) as *const *const u8) };
            let off = unsafe { (substr.add(SUBSTR_OFFSET_OFF) as *const u64).read() } as usize;
            let latin1 =
                unsafe { (*(parent as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0 };
            let stride = if latin1 { 1 } else { 2 };
            let bytes = unsafe {
                core::slice::from_raw_parts(parent.add(STR_DATA_OFF + off * stride), units * stride)
            };
            out.push(bytes.to_vec());
        }
        out
    }

    fn make_utf16(units: &[u16]) -> *mut u8 {
        let length = units.len() as u32;
        let mut b = StrBlock::alloc_with_encoding(length, false);
        let dst = unsafe { b.as_bytes_mut(length * 2) };
        for (i, &u) in units.iter().enumerate() {
            let le = u.to_le_bytes();
            dst[i * 2] = le[0];
            dst[i * 2 + 1] = le[1];
        }
        b.into_raw()
    }

    /// Manually free a split block — drop is normally
    /// __torajs_arr_free dispatched; in tests without that, we
    /// just libc-free the block (the inline substrs share the
    /// same allocation so no separate frees). Also dec the parent
    /// rc once per inline substr to balance init's rc_inc.
    unsafe fn free_split_block(block: *mut u8, parent: *mut u8) {
        let len = unsafe { (block.add(ARR_LEN_OFF) as *const u64).read() } as u64;
        for _ in 0..len as usize {
            unsafe { torajs_rc::__torajs_rc_dec(parent as *mut c_void) };
        }
        // Layer 1 sized free: split-block size is `block_size(len)` —
        // 24 (Arr header) + 40 * len bytes (8 ptr slot + 32 inline
        // substr per element). See `crate::split::pool::block_size`.
        unsafe { free(block as *mut c_void, crate::split::pool::block_size(len)) };
    }

    // Step 4 (v0.7-A2 Phase 2e sweep): Layer 1 sized free.
    unsafe extern "C" {
        #[link_name = "__torajs_free"]
        fn free(ptr: *mut c_void, size: usize);
    }

    #[test]
    fn split_byte_sep_basic() {
        let s = make_str(b"a,b,c");
        let sep = make_str(b",");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_multi_byte_sep() {
        let s = make_str(b"foo<>bar<>baz");
        let sep = make_str(b"<>");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(
            toks,
            vec![b"foo".to_vec(), b"bar".to_vec(), b"baz".to_vec()]
        );
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_empty_sep_per_char() {
        let s = make_str(b"abc");
        let sep = make_str(b"");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_no_match_returns_singleton() {
        let s = make_str(b"abc");
        let sep = make_str(b"z");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![b"abc".to_vec()]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_trailing_empty_token() {
        let s = make_str(b"a,b,");
        let sep = make_str(b",");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![b"a".to_vec(), b"b".to_vec(), b"".to_vec()]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    /// RFC 20260711 follow-up — a UTF-16 haystack splits on the
    /// code-unit grid: `"汉x字x界".split("x")` must answer three
    /// 1-unit tokens (the byte-based shape answered 2/2/2-unit
    /// garbage: byte offsets written into the unit-semantics Substr
    /// fields).
    #[test]
    fn split_utf16_haystack_unit_offsets() {
        let s = make_utf16(&[0x6C49, 0x0078, 0x5B57, 0x0078, 0x754C]);
        let sep = make_str(b"x");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(
            toks,
            vec![vec![0x49u8, 0x6C], vec![0x57u8, 0x5B], vec![0x4Cu8, 0x75]]
        );
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_utf16_no_match_singleton_unit_len() {
        let s = make_utf16(&[0x6C49, 0x754C]);
        let sep = make_str(b"z");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![vec![0x49u8, 0x6C, 0x4C, 0x75]]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_utf16_per_char_units() {
        let s = make_utf16(&[0x6C49, 0x754C]);
        let sep = make_str(b"");
        let block = unsafe { __torajs_str_split(s, sep) };
        let toks = unsafe { read_split_tokens(block) };
        assert_eq!(toks, vec![vec![0x49u8, 0x6C], vec![0x4Cu8, 0x75]]);
        unsafe { free_split_block(block, s) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_iter_utf16_parent_latin1_sep() {
        let s = make_utf16(&[0x6C49, 0x0078, 0x5B57]);
        let sep = make_str(b"x");
        let mut iter: SplitIter = unsafe { core::mem::zeroed() };
        unsafe { __torajs_split_iter_init(&mut iter, s, sep) };
        let mut out = [0u8; 32];
        // token 1: offset 0, 1 unit (汉)
        assert!(unsafe { __torajs_split_iter_next(&mut iter, out.as_mut_ptr()) });
        assert_eq!(unsafe { (out.as_ptr().add(8) as *const u64).read() }, 1);
        assert_eq!(unsafe { (out.as_ptr().add(24) as *const u64).read() }, 0);
        // token 2: offset 2, 1 unit (字)
        assert!(unsafe { __torajs_split_iter_next(&mut iter, out.as_mut_ptr()) });
        assert_eq!(unsafe { (out.as_ptr().add(8) as *const u64).read() }, 1);
        assert_eq!(unsafe { (out.as_ptr().add(24) as *const u64).read() }, 2);
        assert!(!unsafe { __torajs_split_iter_next(&mut iter, out.as_mut_ptr()) });
        unsafe { __torajs_split_iter_drop(&mut iter) };
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }

    #[test]
    fn split_iter_init_and_drop() {
        let s = make_str(b"a,b,c");
        let sep = make_str(b",");
        // Just verify init writes the struct fields + drop decs rc.
        let mut iter: SplitIter = unsafe { core::mem::zeroed() };
        unsafe { __torajs_split_iter_init(&mut iter, s, sep) };
        assert_eq!(iter.parent, s as *const u8);
        assert_eq!(iter.parent_len, 5);
        assert_eq!(iter.sep_len, 1);
        assert_eq!(iter.pos, 0);
        assert_eq!(iter.exhausted, 0);
        // sep_data should be STR_DATA(sep).
        assert_eq!(iter.sep_data, unsafe {
            (sep as *const u8).add(STR_DATA_OFF)
        });
        unsafe { __torajs_split_iter_drop(&mut iter) };
        // Now parent's refcount should be 1 again (the init bump
        // was matched). Free.
        unsafe { __torajs_str_free(s) };
        unsafe { __torajs_str_free(sep) };
    }
}
