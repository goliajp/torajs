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
//!
//! A literal with a compiled matcher for the subject's encoding walks
//! the subject's own code units, so its spans need no mapping; the
//! engine walks the ASCII payload in place or a transcoded copy.

use alloc::vec::Vec;
use core::ffi::c_void;

use super::exec_find::{matcher_for, run_matcher, search_start};
use super::lifecycle::{global_replace_start, tracked_store_refuses};
use super::str_helpers::subject::{UnitsView, str_units_view};
use super::{MatcherFn, RegExp, abort_unsupported, as_regex_mut, haystack, str_slice_ascii_view};
use crate::flags::unicode_mode;
use crate::node::REGEX_MAX_CAPTURES;
use crate::onepass::ir::{is_high_surrogate, is_low_surrogate};
use crate::parser::{RE_FLAG_G, RE_FLAG_Y};
use crate::regex::offset_map::utf16_units_to_byte;
use crate::vm::{MatchResult, match_anchor, save_slot, search_from_with_ws};

/// Bytes the caller reserves for a [`ReplaceIter`]; the lowering
/// allocates this many on its stack.
pub const REPLACE_ITER_SIZE: usize = 96;

pub struct ReplaceIter {
    re: *mut RegExp,
    /// The searched text: the subject's own payload under a matcher
    /// (`s_len` code units, 16-bit when `wide`) or when it is ASCII,
    /// else the transcoded copy in `owned`.
    s_ptr: *const u8,
    s_len: usize,
    owned: Option<Vec<u8>>,
    /// The literal's matcher for the subject's encoding, if any.
    matcher: Option<MatcherFn>,
    /// Position the next search starts at: a byte of the engine's
    /// text, a code unit under a matcher.
    pos: i64,
    /// The UTF-16 unit count at byte `unit_byte`, so a span's units
    /// are counted from the previous match rather than from 0.
    unit_byte: i64,
    unit_at: i64,
    /// Offsets of the searched text are UTF-16 code units already:
    /// under a matcher, or over the ASCII payload.
    unit_offsets: bool,
    wide: bool,
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
    let units = unsafe { str_units_view(str_ptr) };
    let matcher = matcher_for(re, &units);
    // a matcher walks the payload; the engine matches an ASCII
    // subject in place and transcodes the rest once
    let (s_ptr, s_len, unit_offsets, owned) = if matcher.is_some() {
        (units.as_ptr(), units.len(), true, None)
    } else {
        match unsafe { str_slice_ascii_view(str_ptr) } {
            Some(view) => (view.as_ptr(), view.len(), true, None),
            None => {
                let o = unsafe { haystack(re, str_ptr) };
                (o.as_ptr(), o.len(), false, Some(o))
            }
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
            let last = re.last_index_i64();
            pos = if matcher.is_some() {
                search_start(&units, re.flags, last)
            } else {
                // SAFETY: `s_ptr` / `s_len` describe a live buffer.
                let s = unsafe { core::slice::from_raw_parts(s_ptr, s_len) };
                utf16_units_to_byte(s, last, unit_offsets)
            };
            if last > units.len() as i64 {
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
            matcher,
            pos,
            unit_byte: 0,
            unit_at: 0,
            unit_offsets,
            wide: units.wide(),
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
/// or a code point under `u` / `v` — one UTF-8 sequence of the
/// transcoded form, or a surrogate pair of the payload). Without `g`
/// the first hit ends the walk.
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
    let slen = it.s_len as i64;
    let m = if it.pos > slen {
        None
    } else if let Some(f) = it.matcher {
        unsafe { run_matcher(f, re.matcher_stride as usize, &it.units(), it.pos) }
    } else if it.sticky {
        match_anchor(&re.prog, it.bytes(), it.pos, re.flags)
    } else {
        let dfa_view = re.baked_dfa_view();
        let dfa_ref = dfa_view.as_ref().or(re.dfa_runtime.as_ref());
        // the per-RegExp workspace, taken for this search alone: the
        // callback may run the same regex before the next one
        let ws = unsafe { &mut *re.workspace_cache.get() };
        search_from_with_ws(
            &re.prog,
            it.bytes(),
            it.pos,
            re.flags,
            ws,
            dfa_ref,
            it.unit_offsets,
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
            (it.units_at(gs), it.units_at(ge))
        };
        unsafe {
            *out.add(2 * k) = a;
            *out.add(2 * k + 1) = b;
        }
    }
    let end_units = it.units_at(m.end);
    it.unit_byte = m.end;
    it.unit_at = end_units;
    it.pos = if m.end == m.start {
        it.advance(&m)
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
    /// The engine's text.
    fn bytes(&self) -> &[u8] {
        // SAFETY: `init` bound a live buffer of `s_len` bytes.
        unsafe { core::slice::from_raw_parts(self.s_ptr, self.s_len) }
    }

    /// The payload a matcher walks.
    fn units(&self) -> UnitsView<'_> {
        // SAFETY: `init` bound the live payload of `s_len` units.
        unsafe {
            if self.wide {
                UnitsView::Wide(core::slice::from_raw_parts(
                    self.s_ptr as *const u16,
                    self.s_len,
                ))
            } else {
                UnitsView::Narrow(core::slice::from_raw_parts(self.s_ptr, self.s_len))
            }
        }
    }

    /// The position after an empty match at `m.end`.
    fn advance(&self, m: &MatchResult) -> i64 {
        let mut p = m.end + 1;
        let len = self.s_len as i64;
        if self.matcher.is_some() {
            // SAFETY: `init` bound a live regex.
            let flags = unsafe { &*self.re }.flags;
            let units = self.units();
            if self.wide
                && unicode_mode(flags)
                && p < len
                && is_high_surrogate(units.unit(m.end as usize))
                && is_low_surrogate(units.unit(p as usize))
            {
                p += 1;
            }
            return p;
        }
        let s = self.bytes();
        while p < len && s[p as usize] & 0xC0 == 0x80 {
            p += 1;
        }
        p
    }

    /// The UTF-16 unit offset of position `b` of the searched text.
    /// Matches come in order, so most spans are counted from the
    /// previous match end; a capture inside a lookbehind can sit
    /// before it and is counted from the start.
    fn units_at(&self, b: i64) -> i64 {
        if self.unit_offsets {
            return b;
        }
        let s = self.bytes();
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

    /// The walk through a matcher: the reference interpreter over the
    /// program, bound to the regex the way the compiler's function is.
    fn bind_matcher(re: &Re, wide: bool) -> Box<crate::onepass::MatcherIr> {
        let cell = re.cell();
        let mut prog = crate::program::Program::new();
        prog.raw_classes = true;
        let flags = cell.flags;
        let mut p = crate::parser::Parser::new(&cell.src_bytes, flags);
        let root = p.parse().unwrap();
        crate::compiler::compile(&mut prog, &root, flags);
        prog.emit(crate::program::Inst::match_accept());
        let ir = Box::new(crate::onepass::build(&prog, flags, wide).expect("one-pass"));
        cell.matcher_stride = ir.stride;
        ir
    }

    /// A `MatcherFn` over the interpreter: the IR rides in a static
    /// slot because the extern signature has no closure environment.
    static mut TEST_IR: Option<Box<crate::onepass::MatcherIr>> = None;

    unsafe extern "C" fn interp_narrow(
        units: *const u8,
        len: i64,
        start: i64,
        out: *mut i64,
    ) -> i64 {
        let ir = unsafe { (*core::ptr::addr_of!(TEST_IR)).as_ref().unwrap() };
        let s = unsafe { core::slice::from_raw_parts(units, len as usize) };
        let row = unsafe { core::slice::from_raw_parts_mut(out, ir.stride as usize) };
        ir.exec(s, start as usize, row)
    }

    unsafe extern "C" fn interp_wide(units: *const u8, len: i64, start: i64, out: *mut i64) -> i64 {
        let ir = unsafe { (*core::ptr::addr_of!(TEST_IR)).as_ref().unwrap() };
        let s = unsafe { core::slice::from_raw_parts(units as *const u16, len as usize) };
        let row = unsafe { core::slice::from_raw_parts_mut(out, ir.stride as usize) };
        ir.exec(s, start as usize, row)
    }

    fn with_matcher(re: &Re, wide: bool) {
        let ir = bind_matcher(re, wide);
        unsafe { *core::ptr::addr_of_mut!(TEST_IR) = Some(ir) };
        if wide {
            re.cell().matcher_utf16 = Some(interp_wide);
        } else {
            re.cell().matcher_latin1 = Some(interp_narrow);
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

    #[test]
    fn a_matcher_walks_the_payload_of_every_encoding() {
        // "x=é;y=中" through a matcher: spans in units, no transcoding
        let re = Re::new(b"(\\w)=([^;])", b"g");
        with_matcher(&re, false);
        let lat = Cell::latin1(&[b'x', b'=', 0xE9, b';', b'y', b'=', 0xFF]);
        let mut it = Iter::open(&re, &lat);
        assert_eq!(
            it.all(3),
            vec![vec![(0, 3), (0, 1), (2, 3)], vec![(4, 7), (4, 5), (6, 7)]]
        );
        with_matcher(&re, true);
        let wide = Cell::utf16(&[
            b'x' as u16,
            b'=' as u16,
            0x4E2D,
            b';' as u16,
            b'y' as u16,
            b'=' as u16,
            0xD83D,
            0xDE00,
        ]);
        let mut it = Iter::open(&re, &wide);
        assert_eq!(
            it.all(3),
            vec![vec![(0, 3), (0, 1), (2, 3)], vec![(4, 7), (4, 5), (6, 7)]]
        );
        // under `u` the pair is one unit of the class, two of the span
        let re = Re::new(b"(\\w)=([^;])", b"gu");
        with_matcher(&re, true);
        let mut it = Iter::open(&re, &wide);
        assert_eq!(
            it.all(3),
            vec![vec![(0, 3), (0, 1), (2, 3)], vec![(4, 8), (4, 5), (6, 8)]]
        );
        // an empty match under `u` steps over the pair
        let re = Re::new(b"x*", b"gu");
        with_matcher(&re, true);
        let s = Cell::utf16(&[0xD834, 0xDD1E, 0x62]);
        let mut it = Iter::open(&re, &s);
        assert_eq!(it.all(1), vec![vec![(0, 0)], vec![(2, 2)], vec![(3, 3)]]);
    }
}
