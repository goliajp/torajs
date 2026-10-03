//! The dollarless ASCII `replace` in one pass and one allocation —
//! JSC's `jsSpliceSubstringsWithSeparator` shape.
//!
//! [`super::replace`]'s general path re-enters the search per hit and
//! appends every gap and replacement into a per-RegExp `Vec`, which the
//! extern wrapper then copies into a fresh Str. When the haystack and
//! the replacement are both ASCII, the replacement has no `$`, the
//! program is DFA-served and the regex is not sticky, the hits are all
//! the result depends on: walk them with [`dfa_find_from`], add up the
//! length, allocate the Str once and write gaps and replacements into
//! its payload directly.
//!
//! A zero-width hit (it copies one haystack byte and restarts one past
//! it) or more hits than the stack record holds leave the general path
//! to it: `None`.

use core::ffi::c_void;
use core::mem::MaybeUninit;

use crate::vm::search::dfa_find_from;

use super::RegExp;
use super::STR_HDR_SIZE;

unsafe extern "C" {
    fn __torajs_str_alloc_pooled_enc(len: u64, is_latin1: i64) -> *mut u8;
}

/// Hits recorded on the stack before falling back to the general path.
const MAX_HITS: usize = 32;

/// See module doc. `s` and `repl` are ASCII; `repl` has no `$`; the
/// regex is not sticky. Answers a fresh refcount-1 Str.
///
/// # Safety
/// `re` is a live RegExp whose flags the caller already checked.
pub(super) unsafe fn replace_ascii_splice(
    re: &RegExp,
    s: &[u8],
    repl: &[u8],
    global: bool,
) -> Option<*mut c_void> {
    if !(re.prog.can_dfa && crate::dfa::prog_ops_dfa_safe(&re.prog)) {
        return None;
    }
    let dfa_view = re.baked_dfa_view();
    let dfa = dfa_view.as_ref().or(re.dfa_runtime.as_ref())?;
    let slen = s.len() as i64;
    // only `hits[..n]` is ever read, and each of those was written
    // first; zeroing all 512 bytes up front cost more than a short
    // haystack's whole search
    let mut hits = [MaybeUninit::<(usize, usize)>::uninit(); MAX_HITS];
    let mut n = 0;
    let mut removed = 0usize;
    let mut pos: i64 = 0;
    while pos <= slen {
        let Some((st, end)) = dfa_find_from(&re.prog, dfa, s, pos, re.flags, true) else {
            break;
        };
        if end == st || n == MAX_HITS {
            return None;
        }
        hits[n].write((st as usize, end as usize));
        n += 1;
        removed += (end - st) as usize;
        pos = end;
        if !global {
            break;
        }
    }
    let total = s.len() - removed + n * repl.len();
    // SAFETY: the pooled allocator answers a Latin-1 block with room for
    // `total` payload bytes at STR_HDR_SIZE; every byte is written below
    // from in-bounds ranges of `s` and `repl`.
    unsafe {
        let block = __torajs_str_alloc_pooled_enc(total as u64, 1);
        let mut w = block.add(STR_HDR_SIZE);
        let mut prev = 0usize;
        for hit in &hits[..n] {
            let (st, end) = hit.assume_init();
            w = put(w, &s[prev..st]);
            w = put(w, repl);
            prev = end;
        }
        put(w, &s[prev..]);
        Some(block as *mut c_void)
    }
}

/// Copy `src` to `dst`, answering the byte after it.
///
/// The pieces here are gaps and replacements a few bytes long, where a
/// `memcpy` call costs more than the copy. Up to 16 bytes go as two
/// possibly overlapping loads of the widest size that fits, both taken
/// before either store; the overlap rewrites bytes with the same value.
///
/// # Safety
/// `dst` has room for `src.len()` bytes and does not overlap `src`.
#[inline(always)]
pub(super) unsafe fn put(dst: *mut u8, src: &[u8]) -> *mut u8 {
    let n = src.len();
    let p = src.as_ptr();
    // SAFETY: every load stays inside `src[..n]` and every store
    // inside `dst[..n]`; the unaligned accessors carry no alignment
    // requirement.
    unsafe {
        if n >= 8 {
            if n <= 16 {
                let a = (p as *const u64).read_unaligned();
                let b = (p.add(n - 8) as *const u64).read_unaligned();
                (dst as *mut u64).write_unaligned(a);
                (dst.add(n - 8) as *mut u64).write_unaligned(b);
            } else {
                core::ptr::copy_nonoverlapping(p, dst, n);
            }
        } else if n >= 4 {
            let a = (p as *const u32).read_unaligned();
            let b = (p.add(n - 4) as *const u32).read_unaligned();
            (dst as *mut u32).write_unaligned(a);
            (dst.add(n - 4) as *mut u32).write_unaligned(b);
        } else if n >= 2 {
            let a = (p as *const u16).read_unaligned();
            let b = (p.add(n - 2) as *const u16).read_unaligned();
            (dst as *mut u16).write_unaligned(a);
            (dst.add(n - 2) as *mut u16).write_unaligned(b);
        } else if n == 1 {
            *dst = *p;
        }
        dst.add(n)
    }
}

#[cfg(test)]
mod tests {
    use super::put;

    #[test]
    fn put_copies_every_length_exactly() {
        let src: [u8; 40] = core::array::from_fn(|i| i as u8 + 1);
        for n in 0..=src.len() {
            let mut dst = [0xAAu8; 48];
            // SAFETY: `dst` holds 48 bytes, more than any `n` here.
            let end = unsafe { put(dst.as_mut_ptr(), &src[..n]) };
            assert_eq!(end as usize - dst.as_ptr() as usize, n);
            assert_eq!(&dst[..n], &src[..n]);
            assert!(dst[n..].iter().all(|&b| b == 0xAA), "wrote past {n}");
        }
    }
}
