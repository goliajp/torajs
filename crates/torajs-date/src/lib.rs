//! Date class substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate (P6.4, 2026-05-24) — replaces
//! `runtime_date.c` (590 C LOC). Implements the JS Date class:
//!
//! ```text
//! Date (16 bytes)
//!   +0..7   : universal heap header (refcount + type_tag=5 + flags)
//!   +8..15  : ms since UNIX epoch (i64; signed — pre-1970 negative)
//! ```
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`mod@self`] — Date struct + HeapHeader + ABI constants +
//!   cross-tier extern decls + cargo-test stubs.
//! - [`civil`] — Howard-Hinnant `civil_from_days` /
//!   `days_from_civil` (branch-free proleptic Gregorian).
//! - [`tm`] — POSIX `struct tm` mirror + `libc::localtime_r` /
//!   `mktime` FFI + decompose helpers.
//! - [`parse`] — ISO 8601 parser (`YYYY-MM-DDTHH:MM:SS.sssZ`).
//! - [`api`] — public `extern "C"` surface (ctors / getters /
//!   setters / toISOString / toGMTString).

pub mod api;
pub mod civil;
pub mod getters;
pub mod parse;
pub mod tm;

use core::ffi::c_void;

/// Universal heap header (offset 0 of every refcounted heap
/// object). `#[repr(C)]` pins `refcount` at offset 0 for
/// rc_dec / tag-dispatch compat.
#[repr(C)]
pub struct HeapHeader {
    pub refcount: u32,
    pub type_tag: u16,
    pub flags: u16,
}

/// `__TORAJS_TAG_DATE` — heap header tag for Date. Matches
/// runtime_str.c's `value_drop_heap` dispatch on tag=5.
pub const TAG_DATE: u16 = 5;

/// Str heap layout — `__TORAJS_STR_HDR_SIZE` (must match
/// runtime_str.c).
pub const STR_HDR_SIZE: usize = 16;

/// Date sentinel for `parse_iso` failure (caller maps to JS NaN).
pub const DATE_PARSE_FAIL: i64 = i64::MIN;

/// In-memory Date object.
#[repr(C)]
pub struct Date {
    pub header: HeapHeader,
    /// Milliseconds since UNIX epoch (1970-01-01T00:00:00Z).
    pub ms: i64,
}

// ---- Cross-tier extern declarations ----
// Resolved at `tr build` link time against:
//   - libtorajs_rc.a    (__torajs_rc_dec)
//   - libtorajs_str.a / runtime_str.c (__torajs_str_alloc_pooled)
// cargo test substitutes panicking stubs (below).

#[cfg(not(test))]
unsafe extern "C" {
    pub fn __torajs_rc_dec(p: *mut c_void) -> i32;
    pub fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut c_void) -> i32 {
    panic!("torajs-date test stub: __torajs_rc_dec should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-date test stub: __torajs_str_alloc_pooled should not be called from cargo test");
}

/// Lift a `*const c_void` Date pointer to a `&Date`.
///
/// # Safety
///
/// `p` must be non-null and produced by a Date allocator
/// (`__torajs_date_now` / `_from_ms` / etc.); the borrow must not
/// outlive the Date's refcount.
pub unsafe fn as_date<'a>(p: *const c_void) -> &'a Date {
    unsafe { &*(p as *const Date) }
}

/// Lift a `*mut c_void` Date pointer to a `&mut Date` (for the
/// `set_time` / `set_year` mutators).
///
/// # Safety
///
/// Same as [`as_date`]; the borrow is exclusive (no aliases must
/// hold `&Date` concurrently).
pub unsafe fn as_date_mut<'a>(p: *mut c_void) -> &'a mut Date {
    unsafe { &mut *(p as *mut Date) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_layout_matches_c_port() {
        assert_eq!(core::mem::size_of::<Date>(), 16);
        assert_eq!(TAG_DATE, 5);
        assert_eq!(STR_HDR_SIZE, 16);
    }
}
