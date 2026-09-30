//! The replace-callback walk as an iterator the caller drives:
//! `init` binds a regex to a subject, each `next` writes the spans of
//! one more match, `close` releases what `init` took. The compiled
//! loop of `s.replace(re, cb)` runs on it: the callback is called
//! between two `next`s, on views of the subject cut at these spans.
//!
//! What §22.2.6.11 makes observable of lastIndex is kept exactly.
//! Under `g` step 10 stores 0 once, then step 11 collects every match
//! before step 14 runs the first callback — so a callback that reads
//! lastIndex sees 0 and one that writes it changes no match. The
//! iterator therefore stores 0 at `init` and keeps its cursor to
//! itself. Without `g` there is one RegExpBuiltinExec (§22.2.7.2):
//! a sticky pattern anchors at lastIndex and stores the match end, or
//! 0 on a miss, before the callback runs; a plain one never reads it.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::lifecycle::{global_replace_start, tracked_store_refuses};
use super::{RegExp, abort_unsupported, as_regex_mut, haystack, str_slice_ascii_view};
use crate::node::REGEX_MAX_CAPTURES;
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::regex::offset_map::utf16_units_to_byte;
use crate::vm::{match_anchor, save_slot, search_from_with_ws};

/// Bytes the caller reserves for a [`ReplaceIter`]; the lowering
/// allocates this many on its stack.
pub const REPLACE_ITER_SIZE: usize = 80;

pub struct ReplaceIter {
    re: *mut RegExp,
    /// The searched bytes: the subject's own payload when it is ASCII,
    /// else the transcoded copy in `owned`.
    s_ptr: *const u8,
    s_len: usize,
    owned: Option<Vec<u8>>,
    /// Byte position the next search starts at.
    pos: i64,
    /// The UTF-16 unit count at byte `unit_byte`, so a span's units
    /// are counted from the previous match rather than from 0.
    unit_byte: i64,
    unit_at: i64,
    is_ascii: bool,
    global: bool,
    sticky: bool,
    done: bool,
}

const _: () = assert!(core::mem::size_of::<ReplaceIter>() <= REPLACE_ITER_SIZE);

/// Bind `re` to `str_ptr`. Under `g` this is §22.2.6.11 step 10,
/// `Set(rx, "lastIndex", 0)`; a sticky pattern without `g` reads its
/// anchor from lastIndex instead. Either store on a non-writable
/// lastIndex records the TypeError and leaves the iterator exhausted
/// (the caller's throw check follows).
///
/// # Safety
///
/// `iter` points at [`REPLACE_ITER_SIZE`] writable, 8-aligned bytes;
/// `re_ptr` is a live `*RegExp` and `str_ptr` a live `*Str`, both
/// outliving the iterator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_replace_iter_init(
    iter: *mut ReplaceIter,
    re_ptr: *const c_void,
    str_ptr: *const c_void,
) {
    let re = unsafe { as_regex_mut(re_ptr as *mut c_void) };
    if re.rejected != 0 {
        abort_unsupported(re);
    }
    let global = re.flags & RE_FLAG_G != 0;
    let sticky = re.flags & RE_FLAG_Y != 0;
    // an ASCII subject is matched in place; the rest transcode once
    let (s_ptr, s_len, is_ascii, owned) = match unsafe { str_slice_ascii_view(str_ptr) } {
        Some(view) => (view.as_ptr(), view.len(), true, None),
        None => {
            let o = unsafe { haystack(re, str_ptr) };
            (o.as_ptr(), o.len(), false, Some(o))
        }
    };
    let mut pos = 0;
    let mut done = false;
    if global {
        done = !unsafe { global_replace_start(re) };
    } else if sticky {
        if unsafe { tracked_store_refuses(re) } {
            done = true;
        } else {
            // SAFETY: `s_ptr` / `s_len` describe a live buffer.
            let s = unsafe { core::slice::from_raw_parts(s_ptr, s_len) };
            pos = utf16_units_to_byte(s, re.last_index_i64(), is_ascii);
            if pos > s_len as i64 {
                re.set_last_index_num(0.0);
                done = true;
            }
        }
    }
    unsafe {
        iter.write(ReplaceIter {
            re,
            s_ptr,
            s_len,
            owned,
            pos,
            unit_byte: 0,
            unit_at: 0,
            is_ascii,
            global,
            sticky,
            done,
        });
    }
}

/// Find the next match and write the first `n_slots` groups' spans
/// into `out`: `out[2k]` / `out[2k + 1]` are group `k`'s start and
/// end in UTF-16 code units of the subject (`k = 0` is the whole
/// match). A group that did not participate, and every slot past the
/// pattern's capture count, reads -1 in both. Answers 1 on a hit and
/// 0 once the walk is over; `out` is untouched on 0.
///
/// An empty match advances the cursor past one element of the
/// haystack (§22.2.6.11 step 11.c.iv AdvanceStringIndex: a code unit,
/// or a code point under `u` / `v`, which is one UTF-8 sequence of the
/// transcoded form either way). Without `g` the first hit ends the
/// walk.
///
/// # Safety
///
/// `iter` was written by [`__torajs_regex_replace_iter_init`] and not
/// yet closed; `out` points at `2 * n_slots` writable `i64`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_replace_iter_next(
    iter: *mut ReplaceIter,
    out: *mut i64,
    n_slots: i64,
) -> i64 {
    let it = unsafe { &mut *iter };
    if it.done {
        return 0;
    }
    // SAFETY: `init` bound a live regex and a live buffer.
    let re = unsafe { &*it.re };
    let s = unsafe { core::slice::from_raw_parts(it.s_ptr, it.s_len) };
    let slen = s.len() as i64;
    let m = if it.pos > slen {
        None
    } else if it.sticky {
        match_anchor(&re.prog, s, it.pos, re.flags)
    } else {
        let dfa_view = re.baked_dfa_view();
        let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
        // the per-RegExp workspace, taken for this search alone: the
        // callback may run the same regex before the next one
        let ws = unsafe { &mut *re.workspace_cache.get() };
        search_from_with_ws(
            &re.prog,
            s,
            it.pos,
            re.flags,
            ws,
            dfa_ref,
            it.is_ascii,
            true,
        )
    };
    let Some(m) = m else {
        it.done = true;
        if it.sticky && !it.global {
            unsafe { &mut *it.re }.set_last_index_num(0.0);
        }
        return 0;
    };
    let n_cap = (re.n_captures as usize).min(REGEX_MAX_CAPTURES - 1);
    for k in 0..n_slots.max(0) as usize {
        let (gs, ge) = if k == 0 {
            (m.start, m.end)
        } else if k <= n_cap {
            (save_slot(m.saves(), 2 * k), save_slot(m.saves(), 2 * k + 1))
        } else {
            (-1, -1)
        };
        let (a, b) = if gs < 0 || ge < 0 {
            (-1, -1)
        } else {
            (it.units_at(s, gs), it.units_at(s, ge))
        };
        unsafe {
            *out.add(2 * k) = a;
            *out.add(2 * k + 1) = b;
        }
    }
    let end_units = it.units_at(s, m.end);
    it.unit_byte = m.end;
    it.unit_at = end_units;
    it.pos = if m.end == m.start {
        let mut p = m.end + 1;
        while p < slen && s[p as usize] & 0xC0 == 0x80 {
            p += 1;
        }
        p
    } else {
        m.end
    };
    if !it.global {
        it.done = true;
        if it.sticky {
            unsafe { &mut *it.re }.set_last_index_num(end_units as f64);
        }
    }
    1
}

/// Release the transcoded copy of a non-ASCII subject. The ASCII
/// walk owns nothing, so this is free to call on every way out.
///
/// # Safety
///
/// `iter` was written by [`__torajs_regex_replace_iter_init`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regex_replace_iter_close(iter: *mut ReplaceIter) {
    unsafe { (*iter).owned = None };
}

impl ReplaceIter {
    /// The UTF-16 unit offset of byte `b` of the haystack. Matches
    /// come in order, so most spans are counted from the previous
    /// match end; a capture inside a lookbehind can sit before it and
    /// is counted from the start.
    fn units_at(&self, s: &[u8], b: i64) -> i64 {
        if self.is_ascii {
            return b;
        }
        let (from, base) = if b >= self.unit_byte {
            (self.unit_byte, self.unit_at)
        } else {
            (0, 0)
        };
        let end = (b as usize).min(s.len());
        base + s[from as usize..end]
            .iter()
            .map(|&x| ((x & 0xC0) != 0x80) as i64 + (x >= 0xF0) as i64)
            .sum::<i64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec;
    use core::mem::MaybeUninit;

    /// A Str cell for the walk to read: the 16-byte header (refcount,
    /// tag, flags at +6, length at +8) and the payload, 8-aligned.
    struct Cell(Vec<u64>);

    impl Cell {
        fn new(flags: u16, length: u32, payload: &[u8]) -> Cell {
            let mut words = vec![0u64; 2 + payload.len().div_ceil(8)];
            let p = words.as_mut_ptr() as *mut u8;
            unsafe {
                (p as *mut u32).write(1);
                (p.add(6) as *mut u16).write(flags);
                (p.add(8) as *mut u32).write(length);
                core::ptr::copy_nonoverlapping(payload.as_ptr(), p.add(16), payload.len());
            }
            Cell(words)
        }

        fn latin1(payload: &[u8]) -> Cell {
            Cell::new(0x0002, payload.len() as u32, payload)
        }

        fn utf16(units: &[u16]) -> Cell {
            let bytes: Vec<u8> = units.iter().flat_map(|u| u.to_le_bytes()).collect();
            Cell::new(0, units.len() as u32, &bytes)
        }

        fn ptr(&self) -> *const c_void {
            self.0.as_ptr() as *const c_void
        }
    }

    struct Re(*mut c_void);

    impl Re {
        fn new(pat: &[u8], flags: &[u8]) -> Re {
            Re(super::super::compile::compile_bytes(
                pat.to_vec(),
                flags.to_vec(),
            ))
        }

        fn cell(&self) -> &mut RegExp {
            unsafe { as_regex_mut(self.0) }
        }
    }

    impl Drop for Re {
        fn drop(&mut self) {
            unsafe { drop(Box::from_raw(self.0 as *mut RegExp)) };
        }
    }

    struct Iter(MaybeUninit<ReplaceIter>);

    impl Iter {
        fn open(re: &Re, s: &Cell) -> Iter {
            let mut it = Iter(MaybeUninit::uninit());
            unsafe { __torajs_regex_replace_iter_init(it.0.as_mut_ptr(), re.0, s.ptr()) };
            it
        }

        /// The spans of the next match as `(start, end)` pairs for
        /// `n_slots` groups, or `None` when the walk is over.
        fn next(&mut self, n_slots: usize) -> Option<Vec<(i64, i64)>> {
            let mut out = vec![0i64; 2 * n_slots];
            let hit = unsafe {
                __torajs_regex_replace_iter_next(
                    self.0.as_mut_ptr(),
                    out.as_mut_ptr(),
                    n_slots as i64,
                )
            };
            (hit == 1).then(|| out.chunks(2).map(|c| (c[0], c[1])).collect())
        }

        fn all(&mut self, n_slots: usize) -> Vec<Vec<(i64, i64)>> {
            let mut v = Vec::new();
            while let Some(m) = self.next(n_slots) {
                v.push(m);
            }
            v
        }
    }

    impl Drop for Iter {
        fn drop(&mut self) {
            unsafe { __torajs_regex_replace_iter_close(self.0.as_mut_ptr()) };
        }
    }

    #[test]
    fn a_global_walk_stores_zero_once_and_never_reads_last_index() {
        let re = Re::new(b"a", b"g");
        let s = Cell::latin1(b"banana");
        re.cell().set_last_index_num(3.0);
        let mut it = Iter::open(&re, &s);
        assert_eq!(re.cell().last_index, 0.0, "step 10 stored 0");
        // a callback writing lastIndex between two matches changes
        // nothing, and the write survives the walk
        re.cell().set_last_index_num(5.0);
        assert_eq!(it.all(1), vec![vec![(1, 2)], vec![(3, 4)], vec![(5, 6)]]);
        assert_eq!(re.cell().last_index, 5.0);
    }

    #[test]
    fn an_empty_match_advances_one_unit() {
        let re = Re::new(b"x*", b"g");
        let s = Cell::latin1(b"ab");
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.all(1), vec![vec![(0, 0)], vec![(1, 1)], vec![(2, 2)]]);
    }

    #[test]
    fn captures_and_non_participating_groups() {
        let re = Re::new(b"(a)|(b)", b"g");
        let s = Cell::latin1(b"ab");
        let mut it = Iter::open(&re, &s);
        assert_eq!(
            it.all(4),
            vec![
                vec![(0, 1), (0, 1), (-1, -1), (-1, -1)],
                vec![(1, 2), (-1, -1), (1, 2), (-1, -1)],
            ]
        );
    }

    #[test]
    fn a_non_ascii_subject_answers_units_from_its_own_buffer() {
        // "aéb" in Latin-1 and "a中b" in UTF-16: the transcoded copy
        // is wider than the subject, the spans are not
        let re = Re::new(b"b", b"g");
        let lat = Cell::latin1(&[0x61, 0xE9, 0x62]);
        let wide = Cell::utf16(&[0x61, 0x4E2D, 0x62]);
        // two walks over one regex at once, as a callback that runs
        // the same regex would open: each keeps its own copy
        let mut a = Iter::open(&re, &lat);
        let mut b = Iter::open(&re, &wide);
        assert_eq!(a.next(1), Some(vec![(2, 3)]));
        assert_eq!(b.next(1), Some(vec![(2, 3)]));
        assert_eq!(a.next(1), None);
        assert_eq!(b.next(1), None);
    }

    #[test]
    fn an_astral_subject_counts_two_units_per_pair() {
        // "𝄞b" — the pair is two units, so `b` starts at 2
        let re = Re::new(b"b", b"gu");
        let s = Cell::utf16(&[0xD834, 0xDD1E, 0x62]);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.all(1), vec![vec![(2, 3)]]);
        // an empty match under `u` steps over the whole pair
        let re = Re::new(b"", b"gu");
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.all(1), vec![vec![(0, 0)], vec![(2, 2)], vec![(3, 3)]]);
    }

    #[test]
    fn a_sticky_walk_without_g_anchors_at_last_index_and_stores_the_end() {
        let re = Re::new(b"a", b"y");
        let s = Cell::latin1(b"baa");
        re.cell().set_last_index_num(1.0);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.next(1), Some(vec![(1, 2)]));
        assert_eq!(
            re.cell().last_index,
            2.0,
            "§22.2.7.2 step 15 stored the end"
        );
        assert_eq!(it.next(1), None, "one exec without g");
        // a miss stores 0
        re.cell().set_last_index_num(0.0);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.next(1), None);
        assert_eq!(re.cell().last_index, 0.0);
        // past the end is a miss too
        re.cell().set_last_index_num(9.0);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.next(1), None);
        assert_eq!(re.cell().last_index, 0.0);
    }

    #[test]
    fn a_plain_walk_leaves_last_index_alone() {
        let re = Re::new(b"a", b"");
        let s = Cell::latin1(b"baa");
        re.cell().set_last_index_num(2.0);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.all(1), vec![vec![(1, 2)]]);
        assert_eq!(re.cell().last_index, 2.0);
    }
}
