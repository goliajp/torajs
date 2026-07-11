//! `SplitIter` zero-alloc iterator + `__torajs_split_iter_init` /
//! `_next` / `_drop` FFI shims. The 48-byte struct is laid out to
//! match the C `__torajs_split_iter_t` bit-for-bit so the IR-side
//! consumer (or any future external consumer) can read its fields by
//! hardcoded offset. Port of `ssa_inkwell::define_split_iter_next` —
//! sub-step SD-4c gap4 (2026-06-08); LTO across libtorajs_str.a still
//! inlines this body into the for-of caller for OLD-pipeline parity.
//!
//! Extracted from `split/ops.rs` to keep that file under the 500-prod-
//! LOC file-size hard limit (`rules/common/file-size.md`). Pure
//! mechanical pull, no semantic change.

use core::ffi::c_void;

use torajs_rc::__torajs_rc_inc;

use crate::layout::{STR_DATA_OFF, STR_FLAG_IS_LATIN1};
use crate::split::ops::str_len;
use torajs_rc::HeapHeader;

/// 48-byte mirror of the C `__torajs_split_iter_t`. Layout MUST
/// stay bit-for-bit identical — `__torajs_split_iter_next` (Rust
/// port) reads these fields by hardcoded offset.
#[repr(C)]
pub struct SplitIter {
    pub parent: *const u8,   // +0  (8B) — owned ref
    pub parent_len: u64,     // +8  (8B) — cached STR_LEN(parent), code units
    pub sep_data: *const u8, // +16 (8B) — STR_CDATA(sep), borrowed
    pub sep_len: u64,        // +24 (8B) — code units
    pub pos: u64,            // +32 (8B) — current scan position, code units
    pub exhausted: u8,       // +40 (1B)
    /// Cached `sep` encoding (1 = Latin-1) — `next` has no access to
    /// the sep header (only its payload ptr is stored), and the
    /// unit-wise scan needs the stride. Carved out of the pad, so
    /// the 48-byte ABI layout is unchanged.
    pub sep_latin1: u8, //      +41 (1B)
    pub _pad: [u8; 6],       // +42 (6B) — total 48B, 8B aligned
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
        let sep_latin1 = ((*(sep as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0) as u8;
        iter.write(SplitIter {
            parent,
            parent_len: parent_len as u64,
            sep_data,
            sep_len: sep_len as u64,
            pos: 0,
            exhausted: 0,
            sep_latin1,
            _pad: [0; 6],
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
/// Substr layout written at `out` (matches `torajs-str::substr`
/// + `__torajs_substr_*` consumer offsets):
///   +0   u64  header  (FLAG_STATIC_LITERAL << 48)
///   +8   u64  len     (chunk length, code units)
///   +16  ptr  parent  (borrowed Str heap ptr)
///   +24  u64  offset  (code-unit offset into parent's payload)
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
    // Every position / length below is a CODE UNIT value; byte reads
    // recover through each side's encoding stride (the pre-encoding
    // shape scanned bytes, which garbled UTF-16 parents).
    let parent_latin1 = unsafe { (*(parent as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0 };
    let sep_latin1 = it.sep_latin1 != 0;

    #[inline]
    unsafe fn unit_at(data: *const u8, latin1: bool, i: u64) -> u16 {
        unsafe {
            if latin1 {
                *data.add(i as usize) as u16
            } else {
                (data.add((i as usize) * 2) as *const u16).read_unaligned()
            }
        }
    }

    // Resolve (k, len, adv, is_empty_sep).
    let (k_final, len_final, adv, is_empty_sep) = if sep_len == 0 {
        // Empty separator: yield one code unit at a time. Exhaust on
        // entry without emit when pos already at end.
        if pos >= parent_len {
            it.exhausted = 1;
            return false;
        }
        (pos + 1, 1u64, 0u64, true)
    } else if parent_latin1 && sep_latin1 && sep_len == 1 {
        // Latin-1 byte fast path (the dominant static-`" "` shape).
        let target = unsafe { *sep_data };
        let mut k = pos;
        while k < parent_len {
            if unsafe { *parent_bytes.add(k as usize) } == target {
                break;
            }
            k += 1;
        }
        (k, k - pos, 1u64, false)
    } else if parent_latin1 && sep_latin1 {
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
    } else {
        // Mixed / UTF-16 encodings — unit-wise compare on the
        // code-unit grid (a Latin-1 side widens per unit on read).
        let mut mk = pos;
        let k = loop {
            if mk + sep_len > parent_len {
                break parent_len;
            }
            let mut j = 0u64;
            let hit = loop {
                if j >= sep_len {
                    break true;
                }
                let pu = unsafe { unit_at(parent_bytes, parent_latin1, mk + j) };
                let su = unsafe { unit_at(sep_data, sep_latin1, j) };
                if pu != su {
                    break false;
                }
                j += 1;
            };
            if hit {
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
        it.pos = k_final + adv;
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
