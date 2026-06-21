//! `__torajs_str_split` + `__torajs_split_iter_init` / `_drop`.
//!
//! `__torajs_str_split` builds the single-block Arr-with-inline-
//! substrs layout described in [`crate::split`]'s module docs.
//! The pool fast-path lives in [`crate::split::pool`]; this file
//! handles the build (header init + per-segment substr fill) and
//! the iterator surface (`init` / `drop`; `next` is still in IR).
//!
//! Bit-for-bit parity with the pre-rewrite C
//! `__torajs_str_split` is required — the SPLIT_BLOCK + inline-
//! substr layout interacts with the IR-emitted `__torajs_arr_*`
//! free dispatch + the Substr drop chain in ways that any layout
//! drift would break silently.

use core::ffi::c_void;
use core::ptr::NonNull;

use torajs_rc::{__torajs_rc_inc, FLAG_SPLIT_BLOCK, HeapHeader, Tag};

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::split::pool::{self, ARR_HDR_SIZE};
use crate::substr::{
    FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF, SUBSTR_SIZE,
};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u32 {
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
// Pure-Rust cores
// ============================================================

/// Count non-overlapping matches of `sep` in `s`. Used to size
/// the split block. Empty `sep` and `sep.len() > s.len()` are
/// handled by the caller — this fn assumes `1 <= sep.len() <= s.len()`.
///
/// `stride` is 1 for Latin-1 / 2 for UTF-16 so candidate positions
/// stay aligned with the haystack's code-unit grid.
#[inline]
fn count_matches(s: &[u8], sep: &[u8], stride: usize) -> u64 {
    // V0.2 P14-S6 — single-byte-needle SIMD fast path. Latin-1
    // haystack with 1-byte separator (the dominant `" "`, `","`,
    // `"\n"`, `";"` shapes) collapses to a byte-equality reduce
    // that LLVM auto-vectorizes to NEON `pcmpeq + popcount` on
    // ARM64. `bench/cases/split-only-100k` (` `-separated short
    // string) lives on this path.
    if stride == 1 && sep.len() == 1 {
        let target = sep[0];
        return s.iter().filter(|&&b| b == target).count() as u64;
    }
    if sep.len() == stride {
        // Single code-unit needle, UTF-16 path (or anything where
        // sep.len() matches stride exactly): same logic as the
        // 1-byte path but element comparison is multi-byte.
        let mut hits = 0u64;
        let mut i = 0;
        while i + sep.len() <= s.len() {
            if &s[i..i + sep.len()] == sep {
                hits += 1;
            }
            i += stride;
        }
        return hits;
    }
    let limit = s.len() - sep.len();
    let mut hits = 0u64;
    let mut i = 0;
    while i <= limit {
        if &s[i..i + sep.len()] == sep {
            hits += 1;
            i += sep.len();
        } else {
            i += stride;
        }
    }
    hits
}

/// Compute the output token count for `s.split(sep)`.
/// Special cases:
/// - `sep.len() == 0` → code-unit count of `s` (per-char split)
/// - `sep.len() > s.len()` → `1` (no match, whole-s singleton)
/// - otherwise → `count_matches(s, sep, stride) + 1`
#[inline]
pub fn out_count(s: &[u8], sep: &[u8], stride: usize) -> u64 {
    if sep.is_empty() {
        (s.len() / stride) as u64
    } else if sep.len() > s.len() {
        1
    } else {
        count_matches(s, sep, stride) + 1
    }
}

// ============================================================
// Inline substr writer
// ============================================================

/// Initialize one inline substr struct at `substr_slot` and store
/// its address into `*arr_ptr_slot`. Bumps `parent`'s refcount.
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
/// Str heap pointer (the rc_inc call dereferences its header).
#[inline]
unsafe fn split_init_inline(
    substr_slot: *mut u8,
    arr_ptr_slot: *mut *mut u8,
    parent: *const u8,
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
        __torajs_rc_inc(parent as *mut c_void);
        arr_ptr_slot.write(substr_slot);
    }
}

// ============================================================
// Arr header writer
// ============================================================

/// Initialize the Arr header on a fresh split block: refcount=1,
/// tag=Arr, flags=SPLIT_BLOCK, len/cap = `out_count`, head=0.
///
/// # Safety
///
/// `block` must point at a writable region ≥ `ARR_HDR_SIZE`.
#[inline]
unsafe fn write_arr_header(block: NonNull<u8>, out_count: u64) {
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
    }
}

// ============================================================
// extern "C" wrappers
// ============================================================

/// Build the 1-element "no match" block: an Arr containing a
/// single inline Substr that aliases the whole of `s`
/// (offset=0, length=byte_len). Reused by:
///  - `__torajs_str_split` Latin-1-haystack / UTF-16-needle path
///    (match is structurally impossible — emit `[s]` directly)
///  - `__torajs_str_split_no_sep` (separator is `undefined` per
///    ES §22.1.3.21 step 4 — return `[S]`)
///
/// # Safety
///
/// `s` must be a valid Str heap block; `byte_len` its payload byte
/// length.
#[inline]
unsafe fn single_token_block(s: *const u8, byte_len: u64) -> *mut u8 {
    let block = pool::alloc(1);
    unsafe { write_arr_header(block, 1) };
    let slots_size = 8usize;
    let substrs_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE + slots_size) };
    let slots_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };
    unsafe {
        split_init_inline(substrs_base, slots_base, s, 0, byte_len);
    }
    block.as_ptr()
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
            return unsafe { single_token_block(s, s_payload.len() as u64) };
        }
        (false, true) => {
            widened_owned = widen_latin1_to_utf16(sep_payload);
            widened_owned.as_slice()
        }
        _ => sep_payload,
    };
    let sep_byte_len = sep_bytes.len();

    let oc = out_count(s_payload, sep_bytes, stride);
    let block = pool::alloc(oc);
    unsafe { write_arr_header(block, oc) };

    let slots_size = (oc as usize) * 8;
    let substrs_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE + slots_size) };
    let slots_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };

    if sep_len_cu == 0 {
        // Per-char split — emit one Substr per code unit of `s`.
        for k in 0..(s_len_cu as usize) {
            unsafe {
                split_init_inline(
                    substrs_base.add(k * SUBSTR_SIZE),
                    slots_base.add(k),
                    s,
                    (k * stride) as u64,
                    stride as u64,
                );
            }
        }
        return block.as_ptr();
    }

    // Generic path: walk s, emit a substr at every sep boundary,
    // then a trailing substr for [start..s_byte_len].
    let s_byte_len = s_payload.len();
    let mut ix: usize = 0;
    let mut start_byte: usize = 0;
    if stride == 1 && sep_byte_len == 1 && sep_byte_len <= s_byte_len {
        // V0.2 P14-S6 — single-byte-needle SIMD fast path
        // (mirror of `count_matches`'s fast arm). LLVM
        // auto-vectorizes the byte-equality scan to NEON
        // `pcmpeq + popcount` style code on ARM64. Keeps the
        // per-match substr-init body intact (still serial),
        // but eliminates the slice-equality branch + index
        // arithmetic in the gap-scan hot loop.
        let target = sep_bytes[0];
        let mut i = 0;
        while i < s_byte_len {
            if s_payload[i] == target {
                unsafe {
                    split_init_inline(
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
                    split_init_inline(
                        substrs_base.add(ix * SUBSTR_SIZE),
                        slots_base.add(ix),
                        s,
                        start_byte as u64,
                        (i - start_byte) as u64,
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
        split_init_inline(
            substrs_base.add(ix * SUBSTR_SIZE),
            slots_base.add(ix),
            s,
            start_byte as u64,
            (s_byte_len - start_byte) as u64,
        );
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
    let (s_payload, _, _) = unsafe { str_view(s) };
    unsafe { single_token_block(s, s_payload.len() as u64) }
}

// ============================================================
// SplitIter — 48-byte caller-stack struct + init/drop/next fns.
// `__torajs_split_iter_next` was ported from inkwell IR to Rust
// in SD-4c gap4 (2026-06-08) — NEW pipeline has no LLVM backend
// to host the alwaysinline body, and keeping both definitions
// alive would race the linker. LTO across libtorajs_str.a still
// inlines the body into the for-of caller for OLD-pipeline parity.
// ============================================================

/// 48-byte mirror of the C `__torajs_split_iter_t`. Layout MUST
/// stay bit-for-bit identical — `__torajs_split_iter_next` (Rust
/// port) reads these fields by hardcoded offset.
#[repr(C)]
pub struct SplitIter {
    pub parent: *const u8,   // +0  (8B) — owned ref
    pub parent_len: u64,     // +8  (8B) — cached STR_LEN(parent)
    pub sep_data: *const u8, // +16 (8B) — STR_CDATA(sep), borrowed
    pub sep_len: u64,        // +24 (8B)
    pub pos: u64,            // +32 (8B) — current scan position
    pub exhausted: u8,       // +40 (1B)
    pub _pad: [u8; 7],       // +41 (7B) — total 48B, 8B aligned
}

/// Initialize a caller-allocated `SplitIter` over `(parent, sep)`.
/// Bumps `parent`'s refcount (the iter holds one ref); `sep` is
/// borrowed without rc.
///
/// # Safety
///
/// `iter` must point at a writable 48-byte aligned region.
/// `parent` must be a valid Str heap pointer. `sep` must be a
/// valid Str heap pointer that outlives the iter — typically a
/// `FLAG_STATIC_LITERAL` Str (the IR lowering only emits the iter
/// form when sep is a `.rodata` global, so this is naturally
/// satisfied).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_split_iter_init(
    iter: *mut SplitIter,
    parent: *const u8,
    sep: *const u8,
) {
    unsafe {
        let parent_len = str_len(parent);
        let sep_data = (sep).add(STR_DATA_OFF);
        let sep_len = str_len(sep);
        iter.write(SplitIter {
            parent,
            parent_len: parent_len as u64,
            sep_data,
            sep_len: sep_len as u64,
            pos: 0,
            exhausted: 0,
            _pad: [0; 7],
        });
        __torajs_rc_inc(parent as *mut c_void);
    }
}

/// Step the iterator to the next chunk. Returns `true` if a chunk
/// was emitted (and writes a 32-byte Substr layout to `out`),
/// `false` if the iterator was already exhausted on entry. The
/// post-emit "no more separators" case still returns `true` (the
/// tail chunk is yielded); the iter is marked exhausted so the next
/// call returns `false`. Empty-sep mode yields one code-unit per
/// step and falls through naturally when `pos >= parent_len`.
///
/// Port of `ssa_inkwell::define_split_iter_next` — sub-step
/// gap4-port (SD-4c sub-step C iter). NEW pipeline has no LLVM IR
/// backend so the alwaysinline body was a hard gap; LTO across the
/// staticlib still inlines this body into the for-of caller for
/// OLD pipeline parity.
///
/// Substr layout written at `out` (matches `torajs-str::substr`
/// + `__torajs_substr_*` consumer offsets):
///   +0   u64  header  (FLAG_STATIC_LITERAL << 48)
///   +8   u64  len     (chunk byte length)
///   +16  ptr  parent  (borrowed Str heap ptr)
///   +24  u64  offset  (byte offset into parent's payload)
///
/// # Safety
///
/// `iter` must point at a previously [`__torajs_split_iter_init`]
/// -ed `SplitIter`. `out` must point at a writable 32-byte
/// 8-aligned region (caller-stack typical).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_split_iter_next(iter: *mut SplitIter, out: *mut u8) -> bool {
    let it = unsafe { &mut *iter };
    if it.exhausted != 0 {
        return false;
    }
    let parent = it.parent;
    let parent_len = it.parent_len;
    let sep_data = it.sep_data;
    let sep_len = it.sep_len;
    let pos = it.pos;
    let parent_bytes = unsafe { parent.add(STR_DATA_OFF) };

    // Resolve (k, len, stride, is_empty_sep).
    let (k_final, len_final, stride, is_empty_sep) = if sep_len == 0 {
        // Empty separator: yield one byte at a time. Exhaust on entry
        // without emit when pos already at end.
        if pos >= parent_len {
            it.exhausted = 1;
            return false;
        }
        (pos + 1, 1u64, 0u64, true)
    } else if sep_len == 1 {
        let target = unsafe { *sep_data };
        let mut k = pos;
        while k < parent_len {
            if unsafe { *parent_bytes.add(k as usize) } == target {
                break;
            }
            k += 1;
        }
        (k, k - pos, 1u64, false)
    } else {
        let sep_slice = unsafe { core::slice::from_raw_parts(sep_data, sep_len as usize) };
        let mut mk = pos;
        let k = loop {
            if mk + sep_len > parent_len {
                break parent_len;
            }
            let cand = unsafe {
                core::slice::from_raw_parts(parent_bytes.add(mk as usize), sep_len as usize)
            };
            if cand == sep_slice {
                break mk;
            }
            mk += 1;
        };
        (k, k - pos, sep_len, false)
    };

    // Emit Substr to `out`.
    let header_u64: u64 = (crate::symbol::FLAG_STATIC_LITERAL as u64) << 48;
    unsafe {
        (out as *mut u64).write(header_u64);
        (out.add(8) as *mut u64).write(len_final);
        (out.add(16) as *mut *const u8).write(parent);
        (out.add(24) as *mut u64).write(pos);
    }

    // Advance / exhaust decision. Empty-sep path always advances
    // (next call hits the pos>=parent_len early-exit). Scan paths
    // mark exhausted when they hit the end without finding sep.
    if k_final == parent_len && !is_empty_sep {
        it.exhausted = 1;
    } else {
        it.pos = k_final + stride;
    }
    true
}

/// Drop a `SplitIter` — decrements parent's refcount and frees
/// the parent Str block if the iter held the last reference.
///
/// # Safety
///
/// `iter` must point at a previously [`__torajs_split_iter_init`]
/// -ed `SplitIter`. After this call the iter slot must not be
/// re-used without another init.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_split_iter_drop(iter: *mut SplitIter) {
    unsafe {
        let parent = (*iter).parent;
        // The drop is symmetric with init's rc_inc. The rc_dec
        // returns true when refcount hit zero; the actual free path
        // (`__torajs_str_free`) is dispatched by the global rc layer
        // when the rc_dec is performed via the public symbol.
        torajs_rc::__torajs_rc_dec(parent as *mut c_void);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{__torajs_str_free, StrBlock};
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
    /// → STR_DATA, SUBSTR_OFFSET, SUBSTR_LEN).
    unsafe fn read_split_tokens(block: *mut u8) -> Vec<Vec<u8>> {
        let len = unsafe { (block.add(ARR_LEN_OFF) as *const u64).read() } as usize;
        let slots = unsafe { block.add(ARR_DATA_OFF) as *const *mut u8 };
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let substr = unsafe { *slots.add(i) };
            let plen = unsafe { (substr.add(SUBSTR_LEN_OFF) as *const u64).read() } as usize;
            let parent = unsafe { *(substr.add(SUBSTR_PARENT_OFF) as *const *const u8) };
            let off = unsafe { (substr.add(SUBSTR_OFFSET_OFF) as *const u64).read() } as usize;
            let bytes =
                unsafe { core::slice::from_raw_parts(parent.add(STR_DATA_OFF + off), plen) };
            out.push(bytes.to_vec());
        }
        out
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
    fn out_count_paths() {
        assert_eq!(out_count(b"abc", b"", 1), 3); // per-char
        assert_eq!(out_count(b"", b"", 1), 0);
        assert_eq!(out_count(b"abc", b"abcd", 1), 1); // sep longer than s
        assert_eq!(out_count(b"abc", b"z", 1), 1); // no match
        assert_eq!(out_count(b"a,b,c", b",", 1), 3);
        assert_eq!(out_count(b"a,,b", b",", 1), 3); // empty token middle
        assert_eq!(out_count(b",abc,", b",", 1), 3); // empty front+back
        assert_eq!(out_count(b"aaaa", b"aa", 1), 3); // non-overlapping
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
