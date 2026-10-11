//! Wiping secrets so the stores survive optimization.
//!
//! A plain `fill(0)` on a buffer that is never read again is a dead store
//! the optimizer may delete. Volatile writes cannot be removed, and the
//! fence keeps later code from being reordered above them.
#![allow(unsafe_code)]

use core::sync::atomic::{Ordering, compiler_fence};

use super::Word;

/// Overwrites every element of `buf` with zero.
///
/// The writes are volatile, so they are kept even when `buf` is dropped
/// right afterwards.
pub fn zeroize<T: Word>(buf: &mut [T]) {
    for slot in buf.iter_mut() {
        // SAFETY: `slot` is a unique, aligned, initialized reference to a
        // `T`, and `T: Word` is a plain integer with no drop glue, so
        // overwriting it in place is a valid write of a `T`.
        unsafe { core::ptr::write_volatile(slot, T::ZERO) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::zeroize;

    #[test]
    fn clears_every_width() {
        let mut a = [0xffu8; 33];
        let mut b = [u32::MAX; 9];
        let mut c = [u64::MAX; 5];
        zeroize(&mut a);
        zeroize(&mut b);
        zeroize(&mut c);
        assert!(a.iter().all(|&x| x == 0));
        assert!(b.iter().all(|&x| x == 0));
        assert!(c.iter().all(|&x| x == 0));
    }
}
