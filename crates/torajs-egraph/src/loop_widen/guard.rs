//! The growth guards float_demote puts on a sum loop, and the ranges
//! of values they let through.

/// Exponent of the exact-integer range of an f64.
pub(super) const EXACT_BITS: i64 = 53;
const EXACT_MAX: i64 = (1 << EXACT_BITS) - 1;

/// A growth guard: it fires (leaves the fast loop) when its test holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Test {
    /// `v >> k != 0` (logical): `v` outside `[0, 2^k)`
    HighBits(i64),
    Above(i64),
    Below(i64),
}

/// Does every value in `[0, 2^53)` pass the guard?
pub(super) fn passes_exact_range(t: Test) -> bool {
    match t {
        // a register shift takes its amount mod 64, so 64 and up is
        // not the range test it reads as
        Test::HighBits(k) => (EXACT_BITS..64).contains(&k),
        Test::Above(c) => c >= EXACT_MAX,
        Test::Below(c) => c <= 0,
    }
}

/// Is every value that passes the guard within `±2^53`?
pub(super) fn bounds_sum(t: Test) -> bool {
    match t {
        Test::HighBits(k) => (1..=EXACT_BITS).contains(&k),
        Test::Above(c) => c <= EXACT_MAX + 1,
        Test::Below(c) => c >= -EXACT_MAX - 1,
    }
}

/// Does the guard bound the sum from above?
pub(super) fn bounds_sum_above(t: Test) -> bool {
    matches!(t, Test::HighBits(_) | Test::Above(_))
}
