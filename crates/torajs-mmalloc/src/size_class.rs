//! Size classes and the per-class span geometry.
//!
//! Requests up to 4096 bytes round up to a power of two from 16: the
//! small classes, each with a row in the heap's front. Requests up to
//! [`MAX_MEDIUM`] round up to one of four steps per doubling, as
//! mimalloc sizes its medium objects: the medium classes, served
//! straight from their spans. One index space covers both, small
//! first, so a span's class names its size either way. A block of
//! class `c` lives in a span of `span_pages(c)` arena pages carved
//! into `slots_per_span(c)` equal slots. Anything larger routes to
//! `super::large`.

use crate::arena::ARENA_PAGE;

/// Power-of-two size classes served from spans. Requests larger than
/// the last entry route to `super::large`.
pub const SIZE_CLASSES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Number of size classes.
pub const N_CLASSES: usize = SIZE_CLASSES.len();

/// Largest request a small class serves.
pub const MAX_SMALL: usize = SIZE_CLASSES[N_CLASSES - 1];

/// Medium steps per doubling: sizes `5, 6, 7, 8 << k`.
const MEDIUM_STEPS: usize = 4;

/// Doublings the medium classes span, from `MAX_SMALL` up.
const MEDIUM_DOUBLINGS: usize = 8;

/// Medium size classes, four per doubling from `MAX_SMALL`. The last
/// step of the last doubling, 1 MiB, would need more pages than an
/// arena has past its header, so the table stops one short of it.
pub const N_MEDIUM: usize = MEDIUM_STEPS * MEDIUM_DOUBLINGS - 1;

/// Slot sizes of the medium classes, in order.
pub const MEDIUM_CLASSES: [usize; N_MEDIUM] = {
    let mut t = [0; N_MEDIUM];
    let mut i = 0;
    while i < N_MEDIUM {
        let step = (MAX_SMALL / MEDIUM_STEPS) << (i / MEDIUM_STEPS);
        t[i] = step * (MEDIUM_STEPS + 1 + i % MEDIUM_STEPS);
        i += 1;
    }
    t
};

/// Largest request a medium class serves; past it a block maps a
/// region of its own.
pub const MAX_MEDIUM: usize = MEDIUM_CLASSES[N_MEDIUM - 1];

/// Classes in all, small then medium.
pub const N_ALL: usize = N_CLASSES + N_MEDIUM;

const _: () = assert!(MAX_MEDIUM == 896 << 10);
const _: () = assert!(N_ALL <= u8::MAX as usize);

/// A span holds at least this many slots, so the large classes get
/// multi-page spans instead of a few slots per page.
const MIN_SLOTS_PER_SPAN: usize = 32;

/// Round `size` up to its size class index; `None` past the largest
/// class.
///
/// The classes are consecutive powers of two from 16, so the answer
/// is `ceil_log2(size) - 4` clamped at zero: two instructions, no
/// loop. `size_classes_are_the_powers_of_two_this_assumes` pins the
/// assumption; `bucket_for_matches_a_linear_scan` checks every size.
#[inline]
pub const fn bucket_for(size: usize) -> Option<usize> {
    if size > MAX_SMALL {
        return None;
    }
    let ceil_log2 = usize::BITS - size.saturating_sub(1).leading_zeros();
    Some((ceil_log2 as usize).saturating_sub(4))
}

/// Round a request past `MAX_SMALL` up to its medium class, as an
/// index into the shared class space; `None` past `MAX_MEDIUM`.
///
/// With `b = floor(log2(size - 1))`, the two bits below the top one
/// of `size - 1` pick the step inside the doubling: four steps of
/// `2^(b-2)` each, so the class size is `(step + 5) << (b - 2)`.
#[inline]
pub fn medium_class_for(size: usize) -> Option<usize> {
    if size <= MAX_SMALL || size > MAX_MEDIUM {
        return None;
    }
    let top = (usize::BITS - 1 - (size - 1).leading_zeros()) as usize;
    let step = ((size - 1) >> (top - 2)) & (MEDIUM_STEPS - 1);
    Some(N_CLASSES + (top - MAX_SMALL.trailing_zeros() as usize) * MEDIUM_STEPS + step)
}

/// The class of any request a span can serve, small or medium;
/// `None` past `MAX_MEDIUM`.
#[inline]
pub fn class_for(size: usize) -> Option<usize> {
    if size <= MAX_SMALL {
        bucket_for(size)
    } else {
        medium_class_for(size)
    }
}

/// Slot size of class `class`. Out-of-range classes answer 0: callers
/// index with a `bucket_for` answer and `[]` would link
/// `panic_bounds_check` and the integer formatter into every program.
#[inline]
pub fn class_size(class: usize) -> usize {
    match SIZE_CLASSES.get(class) {
        Some(&s) => s,
        None => match MEDIUM_CLASSES.get(class.wrapping_sub(N_CLASSES)) {
            Some(&s) => s,
            None => 0,
        },
    }
}

const fn pages_for(size: usize) -> usize {
    let bytes = size * MIN_SLOTS_PER_SPAN;
    if bytes <= ARENA_PAGE {
        1
    } else {
        bytes.div_ceil(ARENA_PAGE)
    }
}

/// A medium span is at least this long, so the smaller medium
/// classes get several slots per span rather than a span each.
const MIN_MEDIUM_SPAN: usize = 64 << 10;

/// Medium geometry: as many slots as `MIN_MEDIUM_SPAN` holds, at
/// least one, over the pages that takes; a slot that does not divide
/// the page leaves less than a slot of slack at the end.
const fn medium_geometry(size: usize) -> (usize, usize) {
    let want = if size >= MIN_MEDIUM_SPAN {
        1
    } else {
        MIN_MEDIUM_SPAN / size
    };
    let pages = (want * size).div_ceil(ARENA_PAGE);
    (pages, pages * ARENA_PAGE / size)
}

/// Per class: arena pages per span, and slots per span. Computed
/// once here so a class index never meets a `[]` at run time (see
/// `class_size`).
const SPAN_GEOMETRY: [(usize, usize); N_ALL] = {
    let mut t = [(0, 0); N_ALL];
    let mut c = 0;
    while c < N_CLASSES {
        let pages = pages_for(SIZE_CLASSES[c]);
        t[c] = (pages, pages * ARENA_PAGE / SIZE_CLASSES[c]);
        c += 1;
    }
    while c < N_ALL {
        t[c] = medium_geometry(MEDIUM_CLASSES[c - N_CLASSES]);
        c += 1;
    }
    t
};

/// Arena pages per span of `class`: for a small class one page while
/// that holds `MIN_SLOTS_PER_SPAN` slots, more for the larger ones;
/// for a medium class see `medium_geometry`. 0 out of range.
#[inline]
pub fn span_pages(class: usize) -> usize {
    match SPAN_GEOMETRY.get(class) {
        Some(&(pages, _)) => pages,
        None => 0,
    }
}

/// Slots in a span of `class`. 0 out of range.
#[inline]
pub fn slots_per_span(class: usize) -> usize {
    match SPAN_GEOMETRY.get(class) {
        Some(&(_, slots)) => slots,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_routing() {
        assert_eq!(bucket_for(1), Some(0));
        assert_eq!(bucket_for(16), Some(0));
        assert_eq!(bucket_for(17), Some(1));
        assert_eq!(bucket_for(4096), Some(8));
        assert_eq!(bucket_for(4097), None);
    }

    /// The bit-math `bucket_for` is only correct while the classes
    /// are consecutive powers of two starting at 16.
    #[test]
    fn size_classes_are_the_powers_of_two_this_assumes() {
        for (i, &c) in SIZE_CLASSES.iter().enumerate() {
            assert_eq!(c, 16usize << i, "class {i}");
        }
    }

    #[test]
    fn bucket_for_matches_a_linear_scan() {
        let linear = |size: usize| -> Option<usize> {
            if size == 0 {
                return Some(0);
            }
            SIZE_CLASSES.iter().position(|&c| size <= c)
        };
        for size in 0..=(MAX_SMALL + 64) {
            assert_eq!(bucket_for(size), linear(size), "size {size}");
        }
    }

    #[test]
    fn class_size_answers_zero_out_of_range() {
        assert_eq!(class_size(0), 16);
        assert_eq!(class_size(8), 4096);
        assert_eq!(class_size(N_CLASSES), 5 << 10);
        assert_eq!(class_size(N_ALL - 1), MAX_MEDIUM);
        assert_eq!(class_size(N_ALL), 0);
        assert_eq!(class_size(usize::MAX), 0);
    }

    #[test]
    fn medium_classes_are_four_steps_per_doubling() {
        assert_eq!(N_MEDIUM, 31);
        let first: Vec<usize> = [5, 6, 7, 8, 10, 12, 14, 16]
            .iter()
            .map(|k| k << 10)
            .collect();
        assert_eq!(&MEDIUM_CLASSES[..8], &first[..]);
        assert_eq!(MEDIUM_CLASSES[N_MEDIUM - 1], 896 << 10);
        for w in MEDIUM_CLASSES.windows(2) {
            assert!(w[0] < w[1]);
            assert!(
                w[1] * 4 <= w[0] * 5,
                "step {} -> {} wider than 1.25x",
                w[0],
                w[1]
            );
            assert_eq!(w[0] % 16, 0);
        }
    }

    #[test]
    fn medium_class_for_matches_a_linear_scan() {
        let linear = |size: usize| -> Option<usize> {
            if size <= MAX_SMALL {
                return None;
            }
            MEDIUM_CLASSES
                .iter()
                .position(|&c| size <= c)
                .map(|i| i + N_CLASSES)
        };
        let sizes = (0..=(MAX_SMALL + 2048)).chain((MAX_SMALL..=MAX_MEDIUM + 4096).step_by(37));
        for size in sizes {
            assert_eq!(medium_class_for(size), linear(size), "size {size}");
        }
        for (i, &c) in MEDIUM_CLASSES.iter().enumerate() {
            assert_eq!(medium_class_for(c), Some(N_CLASSES + i));
            assert_eq!(medium_class_for(c + 1), linear(c + 1));
        }
        assert_eq!(medium_class_for(MAX_SMALL + 1), Some(N_CLASSES));
        assert_eq!(medium_class_for(MAX_MEDIUM), Some(N_ALL - 1));
        assert_eq!(medium_class_for(MAX_MEDIUM + 1), None);
        assert_eq!(medium_class_for(usize::MAX), None);
    }

    #[test]
    fn class_for_joins_the_two_tables() {
        assert_eq!(class_for(1), Some(0));
        assert_eq!(class_for(MAX_SMALL), Some(N_CLASSES - 1));
        assert_eq!(class_for(MAX_SMALL + 1), Some(N_CLASSES));
        assert_eq!(class_for(MAX_MEDIUM), Some(N_ALL - 1));
        assert_eq!(class_for(MAX_MEDIUM + 1), None);
        for c in 0..N_ALL {
            assert_eq!(class_for(class_size(c)), Some(c), "class {c}");
        }
    }

    /// A medium span fits an arena past its header, holds at least
    /// one slot and wastes less than a slot; the smaller classes get
    /// a span of about `MIN_MEDIUM_SPAN`.
    #[test]
    fn medium_geometry_fits_an_arena_with_little_slack() {
        for (i, &size) in MEDIUM_CLASSES.iter().enumerate() {
            let c = N_CLASSES + i;
            let pages = span_pages(c);
            let slots = slots_per_span(c);
            let max_pages = crate::arena::ARENA_PAGES;
            assert!((1..max_pages).contains(&pages), "class {c} pages {pages}");
            assert!(slots >= 1, "class {c}");
            assert!(slots * size <= pages * ARENA_PAGE, "class {c}");
            assert!(pages * ARENA_PAGE - slots * size < size, "class {c} slack");
            if size >= MIN_MEDIUM_SPAN {
                assert_eq!(slots, 1, "class {c}");
                assert_eq!(pages, size.div_ceil(ARENA_PAGE), "class {c}");
            } else {
                assert!(slots >= MIN_MEDIUM_SPAN / size, "class {c}");
            }
        }
        assert_eq!(span_pages(N_CLASSES), 4);
        assert_eq!(slots_per_span(N_CLASSES), 12);
        assert_eq!(span_pages(N_CLASSES + 3), 4);
        assert_eq!(slots_per_span(N_CLASSES + 3), 8);
        assert_eq!(span_pages(N_ALL - 1), 56);
        assert_eq!(slots_per_span(N_ALL - 1), 1);
        assert_eq!(span_pages(N_ALL), 0);
        assert_eq!(slots_per_span(N_ALL), 0);
    }

    /// Every span holds at least `MIN_SLOTS_PER_SPAN` slots, every
    /// slot count fits the span's `u16` counters, and a span never
    /// outgrows an arena's usable pages.
    #[test]
    fn span_geometry_is_whole_pages_with_enough_slots() {
        for (c, &size) in SIZE_CLASSES.iter().enumerate() {
            let pages = span_pages(c);
            let slots = slots_per_span(c);
            assert!((1..64).contains(&pages), "class {c} pages {pages}");
            assert!(slots >= MIN_SLOTS_PER_SPAN, "class {c} slots {slots}");
            assert!(slots <= u16::MAX as usize, "class {c} slots {slots}");
            assert_eq!(slots * size, pages * ARENA_PAGE, "class {c}");
        }
        assert_eq!(span_pages(0), 1);
        assert_eq!(slots_per_span(0), 1024);
        assert_eq!(span_pages(5), 1);
        assert_eq!(span_pages(6), 2);
        assert_eq!(span_pages(8), 8);
        assert_eq!(slots_per_span(8), 32);
    }
}
