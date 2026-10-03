//! `s.replace(re, text)` / `s.replaceAll(re, text)` through the
//! literal's synthesized matcher, over the subject's own code units.
//!
//! The engine path searches a byte view of the subject (transcoding
//! one past ASCII first), runs a second pass for the groups whenever
//! the replacement has a `$`, and builds the result in a byte buffer
//! that is copied into the string at the end. A literal with a matcher
//! for the subject's encoding needs none of that: one call answers the
//! match and its groups, in code units. A Latin-1 subject and
//! replacement are spliced: the hits (and, for a `$` replacement, their
//! groups) are recorded, the length added up and the string allocated
//! once. Any other pair is expanded per hit into a unit buffer. Both
//! expand a `$` through one GetSubstitution (§22.1.3.19.1) walk.
//!
//! Declines (`None`) a subject with no matcher for its encoding, and a
//! `$` replacement under a pattern with named groups: `$<name>` reads
//! the name table, which holds the names as UTF-8.

use core::ffi::c_void;
use core::mem::MaybeUninit;

use super::exec_find::{matcher_for, search_start};
use super::replace_fn_units::{STR_MAX_UNITS, UnitBuf, advance_empty, too_long};
use super::replace_splice::put;
use super::static_keys::FLAG_STATIC_LITERAL;
use super::str_helpers::subject::{UnitsView, str_units_view};
use super::{__torajs_rc_inc, MatcherFn, RegExp, STR_HDR_SIZE};
use crate::node::REGEX_MAX_CAPTURES;
use crate::parser::RE_FLAG_Y;
use crate::vm::result::{INLINE_SLOTS, save_slot};

unsafe extern "C" {
    fn __torajs_str_alloc_pooled_enc(len: u64, is_latin1: i64) -> *mut u8;
}

/// Hits the splice records on the stack; a walk with more takes the
/// unit buffer instead.
const MAX_HITS: usize = 32;

/// See module doc. Runs after the caller's lastIndex checks
/// (§22.2.6.11 step 10, the sticky store); a sticky replace without
/// `g` anchors at lastIndex and stores the match end, or 0 on a miss.
///
/// # Safety
///
/// `re_cell` is a live RegExp; `str_ptr` / `repl_ptr` are live Strs.
/// The answer is a Str the caller owns.
pub(super) unsafe fn replace_with_matcher(
    re_cell: *mut RegExp,
    str_ptr: *const c_void,
    repl_ptr: *const c_void,
    global: bool,
) -> Option<*mut c_void> {
    let re = unsafe { &*re_cell };
    let view = unsafe { str_units_view(str_ptr) };
    let f = matcher_for(re, &view)?;
    let repl = unsafe { str_units_view(repl_ptr) };
    let dollar = match repl {
        UnitsView::Narrow(r) => r.contains(&b'$'),
        UnitsView::Wide(r) => r.contains(&u16::from(b'$')),
    };
    if dollar && re.n_named_captures > 0 {
        return None;
    }
    let sticky_once = !global && re.flags & RE_FLAG_Y != 0;
    let walk = Walk {
        f,
        stride: re.matcher_stride as usize,
        view: &view,
        flags: re.flags,
        global,
        // a match begins with this unit wherever it begins; a sticky
        // matcher tries one position, so there is nothing to skip
        lead: re
            .prog
            .prefix_byte
            .filter(|&b| b < 0x80 && re.flags & RE_FLAG_Y == 0),
        from: if sticky_once {
            search_start(&view, re.flags, re.last_index_i64())
        } else {
            0
        },
    };
    let mut last_end: Option<i64> = None;
    let n_captures = re.n_captures;
    let spliced = match (&view, &repl) {
        (UnitsView::Narrow(s), UnitsView::Narrow(_)) if !dollar || walk.stride <= INLINE_SLOTS => {
            if dollar {
                unsafe {
                    splice_narrow::<true>(&walk, str_ptr, s, &repl, n_captures, &mut last_end)
                }
            } else {
                unsafe { splice_narrow::<false>(&walk, str_ptr, s, &repl, 0, &mut last_end) }
            }
        }
        _ => None,
    };
    let out = match spliced {
        Some(out) => out,
        None => unsafe { build(&walk, str_ptr, &repl, dollar, n_captures, &mut last_end) },
    };
    if sticky_once {
        // §22.2.7.2 steps 12.a.i / 15
        let end = last_end.map_or(0.0, |e| e as f64);
        unsafe { (*re_cell).set_last_index_num(end) };
    }
    Some(out)
}

/// The hits a replace visits, in order.
struct Walk<'a> {
    f: MatcherFn,
    stride: usize,
    view: &'a UnitsView<'a>,
    flags: u8,
    global: bool,
    /// the ASCII unit every match starts with, for a skip to the next
    /// place one can start
    lead: Option<u8>,
    from: i64,
}

impl Walk<'_> {
    /// Call `hit(start, end, row)` for each match, stopping when it
    /// answers false; `row` is the matcher's saves row in code units.
    /// Answers whether the walk ran to its end.
    unsafe fn each(&self, want_row: bool, mut hit: impl FnMut(i64, i64, &[i64]) -> bool) -> bool {
        let len = self.view.len() as i64;
        let mut pos = self.from;
        let mut inline = [-1i64; INLINE_SLOTS];
        let mut wide_row;
        let row: &mut [i64] = if self.stride <= INLINE_SLOTS {
            &mut inline
        } else {
            wide_row = alloc::vec![-1i64; self.stride];
            &mut wide_row
        };
        while pos <= len {
            if let Some(b) = self.lead {
                let Some(at) = find_unit(self.view, pos as usize, b) else {
                    break;
                };
                pos = at as i64;
            }
            // a walk that never reads the groups lets the row keep what
            // the last call left: the matcher writes the slots it reports
            if want_row {
                row.fill(-1);
            }
            let end = unsafe { (self.f)(self.view.as_ptr(), len, pos, row.as_mut_ptr()) };
            if end < 0 {
                break;
            }
            let st = row[0];
            let saves = if want_row {
                &row[..self.stride]
            } else {
                &[][..]
            };
            if !hit(st, end, saves) {
                return false;
            }
            pos = if end == st {
                end + advance_empty(self.view, self.flags, end)
            } else {
                end
            };
            if !self.global {
                break;
            }
        }
        true
    }
}

/// Index of the first unit `b` in `view` at or after `from`. A
/// Latin-1 payload is read a word at a time: a zero byte of
/// `w ^ splat(b)` marks a `b`, and the lowest marked byte is exact
/// (a borrow only ever marks bytes above a real zero). The last
/// partial word is read as the payload's final eight bytes, with the
/// ones already searched forced nonzero so none of them can borrow.
#[inline(always)]
fn find_unit(view: &UnitsView<'_>, from: usize, b: u8) -> Option<usize> {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    let s = match view {
        UnitsView::Narrow(s) => s,
        UnitsView::Wide(s) => {
            return s[from..]
                .iter()
                .position(|&u| u == u16::from(b))
                .map(|k| from + k);
        }
    };
    let splat = LO * u64::from(b);
    let mut i = from;
    while i + 8 <= s.len() {
        // SAFETY: `i + 8 <= s.len()`; the read carries no alignment need
        let w = u64::from_le(unsafe { (s.as_ptr().add(i) as *const u64).read_unaligned() }) ^ splat;
        let marks = w.wrapping_sub(LO) & !w & HI;
        if marks != 0 {
            return Some(i + (marks.trailing_zeros() / 8) as usize);
        }
        i += 8;
    }
    if i >= s.len() {
        return None;
    }
    if s.len() < 8 {
        return s[i..].iter().position(|&c| c == b).map(|k| i + k);
    }
    let base = s.len() - 8;
    // 1..=7 bytes of this word lie before `i`
    let seen = (1u64 << (8 * (i - base))) - 1;
    // SAFETY: `base + 8 == s.len()`
    let w = u64::from_le(unsafe { (s.as_ptr().add(base) as *const u64).read_unaligned() }) ^ splat;
    let w = w | seen;
    let marks = w.wrapping_sub(LO) & !w & HI;
    (marks != 0).then(|| base + (marks.trailing_zeros() / 8) as usize)
}

/// A Latin-1 subject and replacement: record the hits — with their
/// groups when `SUB` says the replacement has a `$` (`groups` is the
/// pattern's group count) — add up the length, allocate once and write gaps and
/// replacements straight into the payload. `None` past [`MAX_HITS`].
unsafe fn splice_narrow<const SUB: bool>(
    walk: &Walk<'_>,
    str_ptr: *const c_void,
    s: &[u8],
    repl: &UnitsView<'_>,
    groups: i32,
    last_end: &mut Option<i64>,
) -> Option<*mut c_void> {
    let mut hits = [MaybeUninit::<(usize, usize)>::uninit(); MAX_HITS];
    let mut rows = [MaybeUninit::<[i64; INLINE_SLOTS]>::uninit(); MAX_HITS];
    let mut n = 0;
    let mut total = s.len();
    let done = unsafe {
        walk.each(SUB, |st, end, saves| {
            if n == MAX_HITS {
                return false;
            }
            hits[n].write((st as usize, end as usize));
            let added = if SUB {
                let mut row = [-1i64; INLINE_SLOTS];
                row[..saves.len()].copy_from_slice(saves);
                let mut count = Count(0);
                substitute(repl, walk.view, st, end, &row, groups, &mut count);
                rows[n].write(row);
                count.0
            } else {
                repl.len()
            };
            total = total - (end - st) as usize + added;
            n += 1;
            true
        })
    };
    if !done {
        return None;
    }
    if n == 0 {
        return Some(unsafe { share(str_ptr) });
    }
    // SAFETY: only `hits[..n]` / `rows[..n]` are read, and each was
    // written above (`rows` whenever `SUB` is set)
    *last_end = Some(unsafe { hits[n - 1].assume_init() }.1 as i64);
    if total > STR_MAX_UNITS {
        return Some(unsafe { too_long() });
    }
    // SAFETY: a Latin-1 block with room for `total` payload units, each
    // written below from in-bounds ranges of `s` and `repl`, both narrow
    unsafe {
        let block = __torajs_str_alloc_pooled_enc(total as u64, 1);
        let mut w = Raw(block.add(STR_HDR_SIZE));
        let mut prev = 0usize;
        for (k, hit) in hits[..n].iter().enumerate() {
            let (st, end) = hit.assume_init();
            w.0 = put(w.0, &s[prev..st]);
            if SUB {
                let row = rows[k].assume_init_ref();
                substitute(repl, walk.view, st as i64, end as i64, row, groups, &mut w);
            } else {
                w.span(repl, 0, repl.len());
            }
            prev = end;
        }
        put(w.0, &s[prev..]);
        Some(block as *mut c_void)
    }
}

/// Where [`substitute`] writes: a span of a subject or replacement.
trait Sink {
    fn span(&mut self, v: &UnitsView<'_>, from: usize, to: usize);
}

impl Sink for UnitBuf {
    fn span(&mut self, v: &UnitsView<'_>, from: usize, to: usize) {
        self.push(v, from, to);
    }
}

/// Counts the units a substitution writes, up to one past
/// [`STR_MAX_UNITS`].
struct Count(usize);

impl Sink for Count {
    fn span(&mut self, _: &UnitsView<'_>, from: usize, to: usize) {
        // `$&` / `` $` `` / `$'` repeat whole spans, so an uncapped
        // count over a long replacement and subject can pass usize
        self.0 = (self.0 + (to - from)).min(STR_MAX_UNITS + 1);
    }
}

/// Writes Latin-1 units at a cursor into a payload sized for them.
struct Raw(*mut u8);

impl Sink for Raw {
    #[inline(always)]
    fn span(&mut self, v: &UnitsView<'_>, from: usize, to: usize) {
        let UnitsView::Narrow(s) = v else {
            unreachable!("the splice takes Latin-1 sources only")
        };
        // SAFETY: the splice sized the payload for every span it writes
        self.0 = unsafe { put(self.0, &s[from..to]) };
    }
}

/// The subject itself, one more share: a replace that matched nothing
/// answers a string equal to it, and strings carry no identity.
unsafe fn share(subject: *const c_void) -> *mut c_void {
    let flags = unsafe { *((subject as *const u8).add(6) as *const u16) };
    if flags & FLAG_STATIC_LITERAL == 0 {
        unsafe { __torajs_rc_inc(subject as *mut c_void) };
    }
    subject as *mut c_void
}

/// Any other encoding pair, a `$` replacement, or a splice past
/// [`MAX_HITS`]: build the result in code units.
unsafe fn build(
    walk: &Walk<'_>,
    str_ptr: *const c_void,
    repl: &UnitsView<'_>,
    dollar: bool,
    n_captures: i32,
    last_end: &mut Option<i64>,
) -> *mut c_void {
    let view = walk.view;
    let mut out = UnitBuf::for_subject(view);
    let mut copied = 0usize;
    unsafe {
        walk.each(dollar, |st, end, saves| {
            out.push(view, copied, st as usize);
            if dollar {
                substitute(repl, view, st, end, saves, n_captures, &mut out);
            } else {
                out.push(repl, 0, repl.len());
            }
            copied = end as usize;
            *last_end = Some(end);
            true
        });
    }
    if last_end.is_none() {
        return unsafe { share(str_ptr) };
    }
    out.push(view, copied, view.len());
    unsafe { out.into_str() }
}

/// GetSubstitution (§22.1.3.19.1) for one match, in code units: `$$`,
/// `$&`, `` $` ``, `$'`, `$n` / `$nn` (a two-digit index when it names
/// a group); any other `$` stays literal. Same reading as
/// [`super::replace_subst::expand_repl`] without `$<name>`, which the caller
/// keeps off this path.
fn substitute(
    repl: &UnitsView<'_>,
    s: &UnitsView<'_>,
    st: i64,
    en: i64,
    saves: &[i64],
    n_captures: i32,
    out: &mut impl Sink,
) {
    let digit = |u: u32| (u32::from(b'0')..=u32::from(b'9')).contains(&u);
    let n = repl.len();
    // start of the literal run not yet written
    let mut lit = 0;
    let mut i = 0;
    while i < n {
        if repl.unit(i) != u32::from(b'$') || i + 1 >= n {
            i += 1;
            continue;
        }
        let nxt = repl.unit(i + 1);
        let (from, to, used) = match u8::try_from(nxt).unwrap_or(0) {
            // the run keeps one `$` of the two
            b'$' => {
                out.span(repl, lit, i + 1);
                i += 2;
                lit = i;
                continue;
            }
            b'&' => (st, en, 2),
            b'`' => (0, st, 2),
            b'\'' => (en, s.len() as i64, 2),
            _ if digit(nxt) => {
                let d1 = (nxt - u32::from(b'0')) as i32;
                let mut idx = d1;
                let mut used = 2;
                if i + 2 < n && digit(repl.unit(i + 2)) {
                    let two = d1 * 10 + (repl.unit(i + 2) - u32::from(b'0')) as i32;
                    if two >= 1 && two <= n_captures && (two as usize) < REGEX_MAX_CAPTURES {
                        idx = two;
                        used = 3;
                    }
                }
                if !(idx >= 1 && idx <= n_captures && (idx as usize) < REGEX_MAX_CAPTURES) {
                    i += 1;
                    continue;
                }
                let gs = save_slot(saves, 2 * idx as usize);
                let ge = save_slot(saves, 2 * idx as usize + 1);
                if gs >= 0 && ge >= 0 {
                    (gs, ge, used)
                } else {
                    (0, 0, used)
                }
            }
            _ => {
                i += 1;
                continue;
            }
        };
        out.span(repl, lit, i);
        out.span(s, from as usize, to as usize);
        i += used;
        lit = i;
    }
    out.span(repl, lit, n);
}

#[cfg(test)]
mod tests {
    use super::{UnitBuf, UnitsView, find_unit, substitute};
    use alloc::vec::Vec;

    #[test]
    fn find_unit_agrees_with_a_byte_scan() {
        let s: Vec<u8> = (0..40u8)
            .map(|i| if i % 7 == 3 { b'a' } else { b'x' })
            .collect();
        for len in 0..=s.len() {
            let view = UnitsView::Narrow(&s[..len]);
            for from in 0..=len {
                let want = s[from..len]
                    .iter()
                    .position(|&c| c == b'a')
                    .map(|k| from + k);
                assert_eq!(find_unit(&view, from, b'a'), want, "len {len} from {from}");
            }
        }
        // a borrow out of a real match must not mark the byte above it
        let s = [0x61u8, 0x60, 0x61, 0, 0, 0, 0, 0, 0x61];
        assert_eq!(find_unit(&UnitsView::Narrow(&s), 1, 0x61), Some(2));
        // nor may a searched byte of the final word
        let s = *b"xxxxxxxa`";
        assert_eq!(find_unit(&UnitsView::Narrow(&s), 8, b'a'), None);
        let w: Vec<u16> = "xx\u{3042}a".encode_utf16().collect();
        assert_eq!(find_unit(&UnitsView::Wide(&w), 0, b'a'), Some(3));
    }

    // subject "xabbcy", match "abbc" at 1..5, group 1 "bb" at 2..4,
    // group 2 unset
    fn expand(repl: &str) -> Vec<u8> {
        let mut out = UnitBuf::Narrow(Vec::new());
        let saves = [1, 5, 2, 4, -1, -1];
        let (rv, sv) = (
            UnitsView::Narrow(repl.as_bytes()),
            UnitsView::Narrow(b"xabbcy"),
        );
        substitute(&rv, &sv, 1, 5, &saves, 2, &mut out);
        match out {
            UnitBuf::Narrow(v) => v,
            UnitBuf::Wide(_) | UnitBuf::TooLong => unreachable!(),
        }
    }

    #[test]
    fn substitution_reads_every_dollar_form() {
        assert_eq!(expand("<$1>"), b"<bb>");
        assert_eq!(expand("$&|$`|$'"), b"abbc|x|y");
        assert_eq!(expand("$$1"), b"$1");
        assert_eq!(expand("$2."), b".");
        assert_eq!(expand("$01$10"), b"bbbb0");
        assert_eq!(expand("$0$3$x$"), b"$0$3$x$");
        assert_eq!(expand("$<n>"), b"$<n>");
    }
}
