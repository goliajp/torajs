//! Per-field Date getters — port of `runtime_date.c` L418-525.
//!
//! 16 single-line trampolines split by axis:
//! - UTC: `get_utc_full_year / month / date / hours / minutes /
//!   seconds / milliseconds / day` — branch-free arithmetic via
//!   [`crate::civil::civil_from_days`].
//! - LOCAL: same 8 fns, names without `utc_`, via libc
//!   `localtime_r` so the result honors the TZ env var (matches
//!   bun + every other JS engine).

use core::ffi::c_void;

use crate::civil::civil_from_days;
use crate::tm::localtime_decompose;
use crate::{Date, as_date};

/// Decompose `ms` (UNIX ms) into a pure-UTC `(y, m, d, h, min,
/// sec, milli)` 7-tuple. Used by every UTC getter.
pub fn decompose(ms: i64) -> (i32, u32, u32, i32, i32, i32, i32) {
    let day_ms = 86_400_000i64;
    let days = ms.div_euclid(day_ms);
    let mut tod = ms - days * day_ms;
    if tod < 0 {
        tod += day_ms;
    }
    let (y, m, d) = civil_from_days(days);
    let hour = (tod / 3_600_000) as i32;
    let mut rem = tod - hour as i64 * 3_600_000;
    let minute = (rem / 60_000) as i32;
    rem -= minute as i64 * 60_000;
    let second = (rem / 1000) as i32;
    let milli = (rem - second as i64 * 1000) as i32;
    (y, m, d, hour, minute, second, milli)
}

#[inline]
fn date_ms(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        0
    } else {
        unsafe { (*(d_ptr as *const Date)).ms }
    }
}

// ---- UTC getters ----

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_full_year(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).0 as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_month(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).1 as i64 - 1
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_date(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).2 as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_hours(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).3 as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_minutes(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).4 as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_seconds(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).5 as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_milliseconds(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    decompose(unsafe { as_date(d_ptr) }.ms).6 as i64
}

/// `.getUTCDay()` — Sun=0..Sat=6 from days-since-epoch + Thu=4 offset.
///
/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_utc_day(d_ptr: *const c_void) -> i64 {
    let ms = date_ms(d_ptr);
    if d_ptr.is_null() {
        return 0;
    }
    let day_ms = 86_400_000i64;
    let mut days = ms.div_euclid(day_ms);
    let tod = ms - days * day_ms;
    if tod < 0 {
        days -= 1;
    }
    (days + 4).rem_euclid(7)
}

// ---- LOCAL-time getters (libc localtime_r) ----

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_full_year(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_year as i64 + 1900
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_month(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_mon as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_date(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_mday as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_hours(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_hour as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_minutes(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_min as i64
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_seconds(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_sec as i64
}

/// Sub-second milli — timezone-invariant; bypass libc.
///
/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_milliseconds(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    unsafe { as_date(d_ptr) }.ms.rem_euclid(1000)
}

/// # Safety
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_day(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    localtime_decompose(unsafe { as_date(d_ptr) }.ms).tm_wday as i64
}
