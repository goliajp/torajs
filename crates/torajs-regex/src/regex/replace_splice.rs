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
    let mut hits = [(0usize, 0usize); MAX_HITS];
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
        hits[n] = (st as usize, end as usize);
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
        for &(st, end) in &hits[..n] {
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
/// # Safety
/// `dst` has room for `src.len()` bytes.
#[inline(always)]
unsafe fn put(dst: *mut u8, src: &[u8]) -> *mut u8 {
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
        dst.add(src.len())
    }
}
