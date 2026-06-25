//! `Date` string-formatter FFI shims — `__torajs_date_to_*` family
//! (`toISOString` / `toDateString` / `toLocaleString` / `toGMTString`
//! / `toLocaleDateString` / `toLocaleTimeString`). Extracted from
//! `api.rs` to keep that file under the 500-prod-LOC file-size hard
//! limit (`rules/common/file-size.md`). Pure mechanical pull, no
//! semantic change.

use core::ffi::c_void;

use crate::civil::civil_from_days;
use crate::getters::decompose;
use crate::tm::{Tm, localtime_decompose};
use crate::{__torajs_str_alloc_pooled, STR_HDR_SIZE, as_date};

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

fn alloc_str(s: &str) -> *mut u8 {
    let bytes = s.as_bytes();
    let p = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_HDR_SIZE), bytes.len());
        }
    }
    p
}

/// helper — format `"M/D/YYYY"` (en-US locale default,
/// no leading zero on month/day, 4-digit year).
fn fmt_locale_date(tm: &Tm) -> String {
    format!("{}/{}/{:04}", tm.tm_mon + 1, tm.tm_mday, tm.tm_year + 1900)
}

/// helper — format `"h:mm:ss AM/PM"` (en-US locale default,
/// 12-hour clock, no leading zero on hour, 2-digit minute/second).
fn fmt_locale_time(tm: &Tm) -> String {
    let h24 = tm.tm_hour;
    let (h12, ampm) = match h24 {
        0 => (12, "AM"),
        1..=11 => (h24, "AM"),
        12 => (12, "PM"),
        _ => (h24 - 12, "PM"),
    };
    format!("{}:{:02}:{:02} {}", h12, tm.tm_min, tm.tm_sec, ampm)
}

/// `.toISOString()` → `YYYY-MM-DDTHH:MM:SS.sssZ` (UTC).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_iso_string(d_ptr: *const c_void) -> *mut u8 {
    let ms = if d_ptr.is_null() {
        0
    } else {
        unsafe { as_date(d_ptr) }.ms
    };
    let (y, m, d, hour, minute, second, milli) = decompose(ms);
    let s = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y, m, d, hour, minute, second, milli
    );
    let bytes = s.as_bytes();
    let p = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_HDR_SIZE), bytes.len());
        }
    }
    p
}

/// `.toDateString()` per ES §21.4.4.35 — date-only summary like
/// `"Thu Jan 01 1970"`. Uses the local-time decomposition (same
/// helper as `getFullYear` / `getMonth` / `getDate` / `getDay`)
/// so the formatted day matches bun on whichever host timezone
/// the build runs in.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_date_string(d_ptr: *const c_void) -> *mut u8 {
    if d_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(ms);
    let s = format!(
        "{} {} {:02} {:04}",
        DAY_NAMES[tm.tm_wday as usize],
        MONTH_NAMES[tm.tm_mon as usize],
        tm.tm_mday,
        tm.tm_year + 1900,
    );
    let bytes = s.as_bytes();
    let p = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_HDR_SIZE), bytes.len());
        }
    }
    p
}

/// `.toLocaleString()` per ES §21.4.4.39 — en-US default locale,
/// `"M/D/YYYY, h:mm:ss AM/PM"` joined by `", "`. Uses local-time
/// decomposition so the formatted moment matches bun on whichever
/// host timezone the build runs in.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_locale_string(d_ptr: *const c_void) -> *mut u8 {
    if d_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(ms);
    let s = format!("{}, {}", fmt_locale_date(&tm), fmt_locale_time(&tm));
    alloc_str(&s)
}

/// `.toLocaleDateString()` per ES §21.4.4.38 — en-US default locale,
/// `"M/D/YYYY"`.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_locale_date_string(d_ptr: *const c_void) -> *mut u8 {
    if d_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(ms);
    alloc_str(&fmt_locale_date(&tm))
}

/// `.toLocaleTimeString()` per ES §21.4.4.40 — en-US default locale,
/// `"h:mm:ss AM/PM"`.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_locale_time_string(d_ptr: *const c_void) -> *mut u8 {
    if d_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(ms);
    alloc_str(&fmt_locale_time(&tm))
}

/// annexB `.toGMTString()` = `.toUTCString()` → `Wed, 14 Jun 2017
/// 07:00:00 GMT`.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_gmt_string(d_ptr: *const c_void) -> *mut u8 {
    if d_ptr.is_null() {
        return unsafe { __torajs_str_alloc_pooled(0) };
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let day_ms = 86_400_000i64;
    let mut days = ms.div_euclid(day_ms);
    let mut tod = ms - days * day_ms;
    if tod < 0 {
        tod += day_ms;
        days -= 1;
    }
    let (y, m, d) = civil_from_days(days);
    let hour = tod / 3_600_000;
    let mut rem = tod - hour * 3_600_000;
    let minute = rem / 60_000;
    rem -= minute * 60_000;
    let second = rem / 1000;
    let dow = ((days % 7) + 4 + 7) % 7;
    let s = format!(
        "{}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
        DAY_NAMES[dow as usize],
        d,
        MONTH_NAMES[(m - 1) as usize],
        y,
        hour,
        minute,
        second
    );
    let bytes = s.as_bytes();
    let p = unsafe { __torajs_str_alloc_pooled(bytes.len() as u64) };
    if !p.is_null() {
        unsafe {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), p.add(STR_HDR_SIZE), bytes.len());
        }
    }
    p
}
