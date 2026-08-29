//! Date class substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate (P6.4, 2026-05-24) — replaces
//! `runtime_date.c` (590 C LOC). Implements the JS Date class:
//!
//! ```text
//! Date (24 bytes)
//!   +0..7   : universal heap header (refcount + type_tag=5 + flags)
//!   +8..15  : ms since UNIX epoch (i64; signed — pre-1970 negative)
//!   +16..23 : props (*DynObj) — lazy own-property bag, NULL until
//!             the first `d.zz = 1`
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
pub mod api_strings;
pub mod civil;
pub mod getters;
pub mod make_time;
pub mod parse;
pub mod setters_utc;
pub mod subclass;
pub mod tm;
pub mod tz;
pub mod tz_names;

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

/// Invalid-date sentinel — the i64 stand-in for the spec's NaN time
/// value ([[DateValue]] = NaN, RFC 20260713-date-invalid-time). The
/// valid time range |t| ≤ 8.64e15 (TimeClip) can never collide.
pub const DATE_INVALID: i64 = i64::MIN;

/// In-memory Date object.
#[repr(C)]
pub struct Date {
    pub header: HeapHeader,
    /// Milliseconds since UNIX epoch (1970-01-01T00:00:00Z).
    pub ms: i64,
    /// Lazy own-property bag — a Date instance is an ordinary
    /// object (§21.4.4) whose [[DateValue]] is INTERNAL state, so
    /// `d.zz = 1` is an ordinary own property and must land
    /// somewhere. NULL until the first such write; the same
    /// lazily-allocated DynObj shape Promise / wrapper / buffer
    /// cells carry (see [`DATE_PROPS_OFF`]).
    pub props: *mut c_void,
}

/// Byte offset of [`Date::props`] — mirrored by torajs-anyvalue
/// (`member_get_layout::DATE_PROPS_OFF`) and torajs-meta, the same
/// narrow-ABI constant replication [`STR_HDR_SIZE`] uses.
pub const DATE_PROPS_OFF: usize = 16;

// ---- Cross-tier extern declarations ----
// Resolved at `tr build` link time against:
//   - libtorajs_rc.a    (__torajs_rc_dec)
//   - libtorajs_str.a / runtime_str.c (__torajs_str_alloc_pooled)
// cargo test substitutes panicking stubs (below).

#[cfg(not(test))]
unsafe extern "C" {
    pub fn __torajs_rc_dec(p: *mut c_void) -> i32;
    pub fn __torajs_str_alloc_pooled(len: u64) -> *mut u8;
    pub fn __torajs_throw_range_error(msg: *const u8);
    /// torajs-value-drop — universal tag dispatch; releases the
    /// own-property bag when a Date dies.
    pub fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — cycle-root buffer push / scrub (rationale in
    /// `torajs-cycle::buffer`). The push is gated on
    /// `has_walkable_children`, so a bagless cell pays a tag test.
    pub fn __torajs_cycle_buffer(p: *mut c_void);
    pub fn __torajs_cycle_unbuffer(p: *mut c_void);
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_rc_dec(_p: *mut c_void) -> i32 {
    panic!("torajs-date test stub: __torajs_rc_dec should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_value_drop_heap(_p: *mut c_void) {
    panic!("torajs-date test stub: __torajs_value_drop_heap should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_buffer(_p: *mut c_void) {
    panic!("torajs-date test stub: __torajs_cycle_buffer should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_cycle_unbuffer(_p: *mut c_void) {
    panic!("torajs-date test stub: __torajs_cycle_unbuffer should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_str_alloc_pooled(_len: u64) -> *mut u8 {
    panic!("torajs-date test stub: __torajs_str_alloc_pooled should not be called from cargo test");
}

#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_throw_range_error(_msg: *const u8) {
    panic!(
        "torajs-date test stub: __torajs_throw_range_error should not be called from cargo test"
    );
}

// tz.rs's TZ probe (torajs-process at link time) — the cargo-test
// stub answers "TZ unset" so the tests exercise the /etc/localtime
// path exactly as before.
#[cfg(test)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_env_lookup_raw(
    _name: *const u8,
    _name_len: i64,
    _out_len: *mut i64,
) -> *const u8 {
    core::ptr::null()
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
        assert_eq!(core::mem::size_of::<Date>(), 24);
        assert_eq!(DATE_PROPS_OFF, 16);
        assert_eq!(TAG_DATE, 5);
        assert_eq!(STR_HDR_SIZE, 16);
    }
}
