#![forbid(unsafe_code)]

use core::fmt;
use core::hint::black_box;
use core::ops::{BitAnd, BitOr, Not};

/// A secret boolean: `1` for true, `0` for false.
///
/// It is consumed through masks ([`ct_select`], [`cmov`]) rather than
/// `if`. Converting it to `bool` with [`Choice::reveal`] is the point where
/// the value stops being secret.
#[derive(Clone, Copy)]
pub struct Choice(u8);

impl Choice {
    /// True.
    pub const TRUE: Choice = Choice(1);
    /// False.
    pub const FALSE: Choice = Choice(0);

    /// Builds a `Choice` from the low bit of `bit`; the other bits are
    /// ignored.
    #[inline]
    pub fn from_bit(bit: u8) -> Choice {
        Choice(black_box(bit & 1))
    }

    /// Returns the value as `0` or `1`.
    #[inline]
    pub fn to_u8(self) -> u8 {
        self.0
    }

    /// Declassifies the value. Call this only once the result is allowed
    /// to influence control flow, for example the final verdict of a tag
    /// comparison.
    #[inline]
    pub fn reveal(self) -> bool {
        black_box(self.0) == 1
    }

    #[inline]
    fn mask<T: Word>(self) -> T {
        T::mask_from_bit(self.0)
    }
}

impl fmt::Debug for Choice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Choice")
    }
}

impl Not for Choice {
    type Output = Choice;
    #[inline]
    fn not(self) -> Choice {
        Choice(black_box(self.0 ^ 1))
    }
}

impl BitAnd for Choice {
    type Output = Choice;
    #[inline]
    fn bitand(self, rhs: Choice) -> Choice {
        Choice(black_box(self.0 & rhs.0))
    }
}

impl BitOr for Choice {
    type Output = Choice;
    #[inline]
    fn bitor(self, rhs: Choice) -> Choice {
        Choice(black_box(self.0 | rhs.0))
    }
}

mod sealed {
    pub trait Sealed {}
}

/// An unsigned integer that can be selected and wiped in constant time.
///
/// Implemented for `u8`, `u16`, `u32` and `u64`; sealed.
pub trait Word: Copy + sealed::Sealed {
    /// The zero value.
    const ZERO: Self;
    #[doc(hidden)]
    fn mask_from_bit(bit: u8) -> Self;
    #[doc(hidden)]
    fn blend(mask: Self, a: Self, b: Self) -> Self;
}

macro_rules! word {
    ($($t:ty),*) => {$(
        impl sealed::Sealed for $t {}
        impl Word for $t {
            const ZERO: $t = 0;
            #[inline]
            fn mask_from_bit(bit: u8) -> $t {
                (0 as $t).wrapping_sub(bit as $t)
            }
            #[inline]
            fn blend(mask: $t, a: $t, b: $t) -> $t {
                b ^ (mask & (a ^ b))
            }
        }
    )*};
}
word!(u8, u16, u32, u64);

/// Returns `a` when `choice` is true and `b` otherwise, without branching
/// on `choice`.
#[inline]
pub fn ct_select<T: Word>(choice: Choice, a: T, b: T) -> T {
    T::blend(choice.mask(), a, b)
}

/// Copies `src` into `dst` when `choice` is true and leaves `dst` alone
/// otherwise. Both paths touch every element.
///
/// # Panics
///
/// Panics if the slices differ in length (lengths are public).
#[inline]
pub fn cmov<T: Word>(choice: Choice, dst: &mut [T], src: &[T]) {
    assert_eq!(dst.len(), src.len(), "cmov: length mismatch");
    let mask = choice.mask::<T>();
    for (d, &s) in dst.iter_mut().zip(src) {
        *d = T::blend(mask, s, *d);
    }
}

/// Compares two byte strings. The running time depends on the lengths only,
/// not on where the contents differ.
///
/// Slices of different lengths compare unequal.
pub fn ct_eq(a: &[u8], b: &[u8]) -> Choice {
    if a.len() != b.len() {
        return Choice::FALSE;
    }
    let mut acc = 0u8;
    for (&x, &y) in a.iter().zip(b) {
        acc |= x ^ y;
    }
    // acc == 0 exactly when the inputs are equal; fold it to one bit
    // without a comparison: (acc | -acc) has its top bit set iff acc != 0
    let acc = black_box(acc) as u16;
    let nonzero = ((acc | acc.wrapping_neg()) >> 15) as u8;
    Choice::from_bit(nonzero ^ 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_detects_any_single_bit_difference() {
        let a = [0x5au8; 40];
        assert!(ct_eq(&a, &a).reveal());
        for i in 0..a.len() {
            for bit in 0..8 {
                let mut b = a;
                b[i] ^= 1 << bit;
                assert!(!ct_eq(&a, &b).reveal(), "byte {i} bit {bit}");
            }
        }
        assert!(!ct_eq(&a, &a[..39]).reveal());
        assert!(ct_eq(&[], &[]).reveal());
    }

    #[test]
    fn select_and_cmov_follow_the_choice() {
        assert_eq!(ct_select(Choice::TRUE, 7u64, 9), 7);
        assert_eq!(ct_select(Choice::FALSE, 7u64, 9), 9);
        assert_eq!(ct_select(Choice::from_bit(3), 1u8, 2), 1);
        assert_eq!(ct_select(Choice::from_bit(2), 1u16, 2), 2);
        let mut d = [1u32, 2, 3];
        cmov(Choice::FALSE, &mut d, &[9, 9, 9]);
        assert_eq!(d, [1, 2, 3]);
        cmov(Choice::TRUE, &mut d, &[7, 8, 9]);
        assert_eq!(d, [7, 8, 9]);
    }

    #[test]
    fn choice_logic() {
        let (t, f) = (Choice::TRUE, Choice::FALSE);
        assert!((t & t).reveal());
        assert!(!(t & f).reveal());
        assert!((t | f).reveal());
        assert!(!(!t).reveal());
        assert_eq!((!f).to_u8(), 1);
        assert_eq!(std::format!("{t:?}"), "Choice");
    }
}
