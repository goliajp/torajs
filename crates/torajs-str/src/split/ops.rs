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

use core::ptr::NonNull;
use std::ffi::c_void;

use torajs_rc::{__torajs_rc_inc, FLAG_SPLIT_BLOCK, HeapHeader, Tag};

use crate::layout::{STR_DATA_OFF, STR_LEN_OFF};
use crate::split::pool::{self, ARR_HDR_SIZE};
use crate::substr::{
    FLAG_SUBSTR_INLINE, SUBSTR_LEN_OFF, SUBSTR_OFFSET_OFF, SUBSTR_PARENT_OFF, SUBSTR_SIZE,
};

// ============================================================
// Layout-aware FFI helpers (sub-module-local)
// ============================================================

#[inline]
unsafe fn str_len(p: *const u8) -> u64 {
    unsafe { (p.add(STR_LEN_OFF) as *const u64).read() }
}

#[inline]
unsafe fn str_bytes<'a>(p: *const u8, len: u64) -> &'a [u8] {
    unsafe { core::slice::from_raw_parts(p.add(STR_DATA_OFF), len as usize) }
}

// ============================================================
// Pure-Rust cores
// ============================================================

/// Count non-overlapping matches of `sep` in `s`. Used to size
/// the split block. Empty `sep` and `sep.len() > s.len()` are
/// handled by the caller — this fn assumes `1 <= sep.len() <= s.len()`.
#[inline]
fn count_matches(s: &[u8], sep: &[u8]) -> u64 {
    if sep.len() == 1 {
        // Hot path: byte scan (most splits are ' ', ',', '\n').
        let b = sep[0];
        let mut hits = 0u64;
        for &c in s {
            if c == b {
                hits += 1;
            }
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
            i += 1;
        }
    }
    hits
}

/// Compute the output token count for `s.split(sep)`.
/// Special cases:
/// - `sep.len() == 0` → `s.len()` (per-char split)
/// - `sep.len() > s.len()` → `1` (no match, whole-s singleton)
/// - otherwise → `count_matches(s, sep) + 1`
#[inline]
pub fn out_count(s: &[u8], sep: &[u8]) -> u64 {
    if sep.is_empty() {
        s.len() as u64
    } else if sep.len() > s.len() {
        1
    } else {
        count_matches(s, sep) + 1
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
        flags: FLAG_SUBSTR_INLINE,
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
    let s_len = unsafe { str_len(s) };
    let sep_len = unsafe { str_len(sep) };
    let s_bytes = unsafe { str_bytes(s, s_len) };
    let sep_bytes = unsafe { str_bytes(sep, sep_len) };

    let oc = out_count(s_bytes, sep_bytes);
    let block = pool::alloc(oc);
    unsafe { write_arr_header(block, oc) };

    let slots_size = (oc as usize) * 8;
    let substrs_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE + slots_size) };
    let slots_base = unsafe { block.as_ptr().add(ARR_HDR_SIZE) as *mut *mut u8 };

    if sep_len == 0 {
        // Per-char split.
        for k in 0..s_len {
            let ku = k as usize;
            unsafe {
                split_init_inline(
                    substrs_base.add(ku * SUBSTR_SIZE),
                    slots_base.add(ku),
                    s,
                    k,
                    1,
                );
            }
        }
        return block.as_ptr();
    }

    // Generic path: walk s, emit a substr at every sep boundary,
    // then a trailing substr for [start..s_len].
    let mut ix: usize = 0;
    let mut start: u64 = 0;
    if sep_len == 1 {
        // Hot path: byte scan.
        let b = sep_bytes[0];
        for (k, &c) in s_bytes.iter().enumerate() {
            if c == b {
                unsafe {
                    split_init_inline(
                        substrs_base.add(ix * SUBSTR_SIZE),
                        slots_base.add(ix),
                        s,
                        start,
                        k as u64 - start,
                    );
                }
                ix += 1;
                start = k as u64 + 1;
            }
        }
    } else if sep_len <= s_len {
        let limit = (s_len - sep_len) as usize;
        let mut i: usize = 0;
        while i <= limit {
            if &s_bytes[i..i + sep_len as usize] == sep_bytes {
                unsafe {
                    split_init_inline(
                        substrs_base.add(ix * SUBSTR_SIZE),
                        slots_base.add(ix),
                        s,
                        start,
                        i as u64 - start,
                    );
                }
                ix += 1;
                i += sep_len as usize;
                start = i as u64;
            } else {
                i += 1;
            }
        }
    }
    // Trailing token (may be empty if s ends with sep).
    unsafe {
        split_init_inline(
            substrs_base.add(ix * SUBSTR_SIZE),
            slots_base.add(ix),
            s,
            start,
            s_len - start,
        );
    }
    block.as_ptr()
}

// ============================================================
// SplitIter — 48-byte caller-stack struct + init/drop fns.
// `__torajs_split_iter_next` body is still in inkwell IR
// (ssa_inkwell::define_split_iter_next) and consolidates into
// Rust in P3.1-g.
// ============================================================

/// 48-byte mirror of the C `__torajs_split_iter_t`. Layout MUST
/// stay bit-for-bit identical — the IR-emitted `_next` body reads
/// these fields by hardcoded offset.
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
            parent_len,
            sep_data,
            sep_len,
            pos: 0,
            exhausted: 0,
            _pad: [0; 7],
        });
        __torajs_rc_inc(parent as *mut c_void);
    }
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
    use crate::alloc::{__torajs_str_free, StrBlock};

    // ARR layout consts (mirror C runtime_str.c — cross-layer until
    // torajs-arr crate lands). Only the offsets the test path reads
    // are declared here; ARR_CAP_OFF lives in split::pool which the
    // production code uses for the free dispatch.
    const ARR_LEN_OFF: usize = 8;
    const ARR_DATA_OFF: usize = ARR_HDR_SIZE;

    fn make_str(payload: &[u8]) -> *mut u8 {
        let mut b = StrBlock::alloc(payload.len() as u64);
        let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
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
        let len = unsafe { (block.add(ARR_LEN_OFF) as *const u64).read() } as usize;
        for _ in 0..len {
            unsafe { torajs_rc::__torajs_rc_dec(parent as *mut c_void) };
        }
        unsafe { free(block as *mut c_void) };
    }

    unsafe extern "C" {
        fn free(ptr: *mut c_void);
    }

    #[test]
    fn out_count_paths() {
        assert_eq!(out_count(b"abc", b""), 3); // per-char
        assert_eq!(out_count(b"", b""), 0);
        assert_eq!(out_count(b"abc", b"abcd"), 1); // sep longer than s
        assert_eq!(out_count(b"abc", b"z"), 1); // no match
        assert_eq!(out_count(b"a,b,c", b","), 3);
        assert_eq!(out_count(b"a,,b", b","), 3); // empty token middle
        assert_eq!(out_count(b",abc,", b","), 3); // empty front+back
        assert_eq!(out_count(b"aaaa", b"aa"), 3); // non-overlapping
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
