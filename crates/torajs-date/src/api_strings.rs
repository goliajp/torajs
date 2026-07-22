//! `Date` string-formatter FFI shims — `__torajs_date_to_*` family
//! (`toISOString` / `toDateString` / `toLocaleString` / `toGMTString`
//! / `toLocaleDateString` / `toLocaleTimeString`). Extracted from
//! `api.rs` to keep that file under the 500-prod-LOC file-size hard
//! limit (`rules/common/file-size.md`). Pure mechanical pull, no
//! semantic change.

use core::ffi::c_void;

use crate::api::valid_ms;
use crate::civil::civil_from_days;
use crate::getters::decompose;
use crate::tm::{Tm, localtime_decompose};
use crate::{__torajs_str_alloc_pooled, __torajs_throw_range_error, STR_HDR_SIZE};

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

/// helper — spec DateString / toUTCString year: abs 4-digit
/// zero-pad with a leading '-' for negative years (§21.4.4.41.2 —
/// year -1 renders "-0001", not the sign-inclusive-width "-001").
fn fmt_year_padded(y: i64) -> String {
    if y < 0 {
        format!("-{:04}", -y)
    } else {
        format!("{:04}", y)
    }
}

/// helper — toISOString year: plain 4-digit for 0..=9999, expanded
/// sign + 6-digit otherwise (§21.4.4.36 / ISO 8601 expanded years:
/// "+275760" / "-271821").
fn fmt_iso_year(y: i64) -> String {
    if (0..=9999).contains(&y) {
        format!("{:04}", y)
    } else if y < 0 {
        format!("-{:06}", -y)
    } else {
        format!("+{:06}", y)
    }
}

/// `.toISOString()` → `YYYY-MM-DDTHH:MM:SS.sssZ` (UTC).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_iso_string(d_ptr: *const c_void) -> *mut u8 {
    let Some(ms) = valid_ms(d_ptr) else {
        // invalid time value → RangeError (§21.4.4.36 step 3); the
        // call-site emit_throw_check propagates before the value is
        // consumed. Message matches bun/JSC ("Invalid Date").
        unsafe { __torajs_throw_range_error(b"Invalid Date\0".as_ptr()) };
        return alloc_str("");
    };
    let (y, m, d, hour, minute, second, milli) = decompose(ms);
    let s = format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        fmt_iso_year(y as i64),
        m,
        d,
        hour,
        minute,
        second,
        milli
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

/// `.toJSON()` per ES §21.4.4.37 — a non-finite time value answers
/// JS null (steps 2-3; NULL in the Str slot's three-shape
/// convention), never toISOString's RangeError; otherwise the ISO
/// string.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is NULL
/// (JS null) or a pooled Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_json(d_ptr: *const c_void) -> *mut u8 {
    if valid_ms(d_ptr).is_none() {
        return core::ptr::null_mut();
    }
    unsafe { __torajs_date_to_iso_string(d_ptr) }
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
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
    let tm = localtime_decompose(ms);
    let s = format!(
        "{} {} {:02} {}",
        DAY_NAMES[tm.tm_wday as usize],
        MONTH_NAMES[tm.tm_mon as usize],
        tm.tm_mday,
        fmt_year_padded(tm.tm_year as i64 + 1900),
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

/// `.toTimeString()` per ES §21.4.4.42 TimeString + TimeZoneString —
/// the time half of `.toString()`: `"09:00:00 GMT+0900 (Japan
/// Standard Time)"`. Same TZif offset + CLDR long-name chain,
/// degrading to `(GMT±HHMM)` when the host zone is untabled.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_time_string(d_ptr: *const c_void) -> *mut u8 {
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
    let tm = localtime_decompose(ms);
    let utc_secs = ms.div_euclid(1000);
    let offs = crate::tz::local_utoff(utc_secs);
    let sign = if offs < 0 { '-' } else { '+' };
    let abs = offs.unsigned_abs();
    let (oh, om) = (abs / 3600, abs / 60 % 60);
    let gmt = format!("GMT{}{:02}{:02}", sign, oh, om);
    let name = crate::tz::zone_long_name(utc_secs);
    let s = format!(
        "{:02}:{:02}:{:02} {} ({})",
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        gmt,
        name.unwrap_or(&gmt),
    );
    alloc_str(&s)
}

/// `.toString()` per ES §21.4.4.41 / §21.4.4.41.2 ToDateString —
/// `"Thu Jan 01 1970 09:00:00 GMT+0900 (Japan Standard Time)"`.
/// Local-time decomposition + the TZif UT offset; the parenthesized
/// long name comes from the CLDR table keyed by the /etc/localtime
/// symlink target (DST-aware), degrading to `(GMT±HHMM)` when the
/// host zone is unknown or untabled.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`. Returned pointer is a pooled
/// Str (rc=1; caller takes ownership).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_to_string(d_ptr: *const c_void) -> *mut u8 {
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
    let tm = localtime_decompose(ms);
    let utc_secs = ms.div_euclid(1000);
    let offs = crate::tz::local_utoff(utc_secs);
    let sign = if offs < 0 { '-' } else { '+' };
    let abs = offs.unsigned_abs();
    let (oh, om) = (abs / 3600, abs / 60 % 60);
    let gmt = format!("GMT{}{:02}{:02}", sign, oh, om);
    let name = crate::tz::zone_long_name(utc_secs);
    let s = format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} {} ({})",
        DAY_NAMES[tm.tm_wday as usize],
        MONTH_NAMES[tm.tm_mon as usize],
        tm.tm_mday,
        fmt_year_padded(tm.tm_year as i64 + 1900),
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec,
        gmt,
        name.unwrap_or(&gmt),
    );
    alloc_str(&s)
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
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
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
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
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
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
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
    let Some(ms) = valid_ms(d_ptr) else {
        return alloc_str("Invalid Date");
    };
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
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        DAY_NAMES[dow as usize],
        d,
        MONTH_NAMES[(m - 1) as usize],
        fmt_year_padded(y as i64),
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
