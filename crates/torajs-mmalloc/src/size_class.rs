//! Size classes and the per-class span geometry.
//!
//! Requests up to 4096 bytes round up to a power of two from 16; a
//! block of class `c` lives in a span of `span_pages(c)` arena pages
//! carved into `slots_per_span(c)` equal slots. Anything larger
//! routes to `super::large`.

use crate::arena::ARENA_PAGE;

/// Power-of-two size classes served from spans. Requests larger than
/// the last entry route to `super::large`.
pub const SIZE_CLASSES: [usize; 9] = [16, 32, 64, 128, 256, 512, 1024, 2048, 4096];

/// Number of size classes.
pub const N_CLASSES: usize = SIZE_CLASSES.len();

/// Largest request a size class serves.
pub const MAX_SMALL: usize = SIZE_CLASSES[N_CLASSES - 1];

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
pub fn bucket_for(size: usize) -> Option<usize> {
    if size > MAX_SMALL {
        return None;
    }
    let ceil_log2 = usize::BITS - size.saturating_sub(1).leading_zeros();
    Some((ceil_log2 as usize).saturating_sub(4))
}

/// Slot size of class `class`. Out-of-range classes answer 0: callers
/// index with a `bucket_for` answer and `[]` would link
/// `panic_bounds_check` and the integer formatter into every program.
#[inline]
pub fn class_size(class: usize) -> usize {
    match SIZE_CLASSES.get(class) {
        Some(&s) => s,
        None => 0,
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

/// Per class: arena pages per span, and slots per span. Computed
/// once here so a class index never meets a `[]` at run time (see
/// `class_size`).
const SPAN_GEOMETRY: [(usize, usize); N_CLASSES] = {
    let mut t = [(0, 0); N_CLASSES];
    let mut c = 0;
    while c < N_CLASSES {
        let pages = pages_for(SIZE_CLASSES[c]);
        t[c] = (pages, pages * ARENA_PAGE / SIZE_CLASSES[c]);
        c += 1;
    }
    t
};

/// Arena pages per span of `class`: one page while that holds
/// `MIN_SLOTS_PER_SPAN` slots, more for the larger classes. 0 out of
/// range.
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
        assert_eq!(class_size(9), 0);
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
        assert_eq!(span_pages(N_CLASSES), 0);
        assert_eq!(slots_per_span(N_CLASSES), 0);
    }
}
