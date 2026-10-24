//! The free of the use-after-free detector build.

/// The word a freed block carries at `FREE_LINK_OFF` under
/// `--cfg torajs_uaf_detect`. The offset is an object cell's class-tag
/// slot, and no class tag is this large, so a typed field read can
/// tell a dead cell from a live one.
pub const UAF_POISON: u64 = 0xdead_f4ee_dead_f4ee;

/// The diagnostic build's free: the block is marked dead and never
/// handed out again, so a read through a stale pointer meets the mark
/// instead of whatever object reused the address. Answers whether the
/// block was kept.
///
/// Only the small size classes that go through the thread row are
/// covered: blocks past `MAX_SMALL` (`free_big` / `free_large` in
/// `core.rs`) are still reused or unmapped as usual.
///
/// # Safety
/// `p` is a live block of at least 16 bytes.
#[cfg(torajs_uaf_detect)]
#[inline(always)]
pub(super) unsafe fn quarantine(p: *mut u8) -> bool {
    unsafe { *(p.add(super::FREE_LINK_OFF) as *mut u64) = UAF_POISON };
    true
}

#[cfg(not(torajs_uaf_detect))]
#[inline(always)]
pub(super) unsafe fn quarantine(_p: *mut u8) -> bool {
    false
}
