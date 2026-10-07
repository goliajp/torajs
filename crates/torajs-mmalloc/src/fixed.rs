//! Layer 1 for a size fixed when the caller is compiled, inlined into
//! another runtime archive: the class is a constant, so the row pop
//! and push are the whole fast path, with no size checks and no call.
//!
//! An inlined copy reaches the heap only through exported symbols —
//! the row through `__torajs_core_tlab`, a miss through the C entries —
//! which every archive resolves to the one allocator at link time;
//! a crate-private path would be internalised per archive.

use core::ffi::c_void;

use crate::core::__torajs_core_tlab;
use crate::size_class::{MAX_SMALL, bucket_for};

/// Size class of a block of `SIZE` bytes; fails to compile for a size
/// no row serves.
struct Fixed<const SIZE: usize>;

impl<const SIZE: usize> Fixed<SIZE> {
    const CLASS: usize = match bucket_for(SIZE) {
        Some(c) if SIZE != 0 && SIZE <= MAX_SMALL => c,
        _ => panic!("no row serves this size"),
    };
}

/// Pop a block of `SIZE` bytes off its row; null when the row is
/// empty, where the caller takes `__torajs_malloc(SIZE)`.
#[inline(always)]
pub fn pop_fixed<const SIZE: usize>() -> *mut u8 {
    // a census build counts every allocation in `alloc_sized`
    if cfg!(torajs_census) {
        return core::ptr::null_mut();
    }
    let tlab = &raw mut __torajs_core_tlab;
    match unsafe { (*tlab).pop(Fixed::<SIZE>::CLASS) } {
        Some(p) => p,
        None => core::ptr::null_mut(),
    }
}

/// A block of `SIZE` bytes: the row, else the allocator's own path.
/// Null on OOM.
#[inline(always)]
pub fn alloc_fixed<const SIZE: usize>() -> *mut u8 {
    let p = pop_fixed::<SIZE>();
    if !p.is_null() {
        return p;
    }
    unsafe { crate::extern_api::__torajs_malloc(SIZE) as *mut u8 }
}

/// Free a block of `SIZE` bytes: a push onto its row, a spill when
/// the row is full.
///
/// # Safety
/// `ptr` came from an allocation of `SIZE` bytes and is not already
/// freed.
#[inline(always)]
pub unsafe fn free_fixed<const SIZE: usize>(ptr: *mut u8) {
    if !cfg!(torajs_census) {
        let tlab = &raw mut __torajs_core_tlab;
        if unsafe { (*tlab).push(Fixed::<SIZE>::CLASS, ptr) } {
            return;
        }
    }
    unsafe { __torajs_free_class(ptr as *mut c_void, Fixed::<SIZE>::CLASS) };
}

/// Free a block of small class `class` without the size checks of
/// `__torajs_free`: a push onto the row, else a spill.
///
/// # Safety
/// `ptr` is a live block of small class `class` from this allocator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_free_class(ptr: *mut c_void, class: usize) {
    #[cfg(torajs_census)]
    crate::core::census::FREES.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let tlab = &raw mut __torajs_core_tlab;
    if unsafe { (*tlab).push(class, ptr as *mut u8) } {
        return;
    }
    unsafe { crate::core::free_spill(ptr as *mut u8, class) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tlab::SPILL_AT;

    /// A fixed-size free lands on the row a sized alloc pops, and a
    /// full row spills instead of refusing the block.
    #[test]
    fn fixed_blocks_share_the_sized_row_and_spill_when_full() {
        let p = alloc_fixed::<64>();
        assert!(!p.is_null());
        unsafe { free_fixed::<64>(p) };
        assert_eq!(crate::core::alloc_sized(64), p);
        let blocks: Vec<*mut u8> = (0..2 * SPILL_AT).map(|_| alloc_fixed::<64>()).collect();
        for &b in &blocks {
            unsafe { free_fixed::<64>(b) };
        }
        let tlab = &raw const __torajs_core_tlab;
        let class = bucket_for(64).unwrap();
        assert!(unsafe { (*tlab).row_len(class) } <= SPILL_AT);
        unsafe { free_fixed::<64>(p) };
    }
}
