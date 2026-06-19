//! Public extern "C" surface — ctors / getters / setters /
//! toISOString / toGMTString. Port of `runtime_date.c` L77-589
//! (excluding civil + tm + parse helpers extracted to siblings).

use core::ffi::c_void;

use crate::civil::civil_from_days;
use crate::getters::decompose;
use crate::parse::parse_iso;
use crate::tm::{Tm, components_to_local_ms, localtime_decompose};
use crate::{
    __torajs_rc_dec, __torajs_str_alloc_pooled, DATE_PARSE_FAIL, Date, HeapHeader, STR_HDR_SIZE,
    TAG_DATE, as_date, as_date_mut,
};

// ---- Time source ----

/// Wall-clock ms since UNIX epoch via a direct `gettimeofday`
/// syscall (`torajs_syscall::gettimeofday`) — metal-level time source
/// that keeps the AOT user binary free of libc `clock_gettime` /
/// `__error` / `strerror_r`. Returns 0 on the (rare) syscall failure.
fn now_ms() -> i64 {
    match torajs_syscall::gettimeofday() {
        Ok((sec, usec)) => sec * 1000 + (usec as i64) / 1000,
        Err(_) => 0,
    }
}

// ---- Constructors ----

fn alloc_date(ms: i64) -> *mut c_void {
    let d = Box::new(Date {
        header: HeapHeader {
            refcount: 1,
            type_tag: TAG_DATE,
            flags: 0,
        },
        ms,
    });
    Box::into_raw(d) as *mut c_void
}

/// `new Date()` — current wall clock.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_now() -> *mut c_void {
    alloc_date(now_ms())
}

/// `new Date(ms)` — from milliseconds since epoch.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_from_ms(ms: i64) -> *mut c_void {
    alloc_date(ms)
}

/// `__torajs_date_components_to_local_ms` — exposed as a free
/// helper because ssa_lower calls it directly for setters.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_components_to_local_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    components_to_local_ms(year, month, day, hour, minute, second, milli)
}

/// `new Date(y, m, d, h, mi, s, ms)` — LOCAL-time interpretation.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_from_components(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> *mut c_void {
    alloc_date(components_to_local_ms(
        year, month, day, hour, minute, second, milli,
    ))
}

/// `Date.UTC(y, m, d, h, mi, s, ms)` — pure UTC interpretation.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_utc_components(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    let year = if (0..100).contains(&year) {
        year + 1900
    } else {
        year
    };
    let mut y = year + month.div_euclid(12);
    let m = month.rem_euclid(12);
    if m < 0 {
        y -= 1;
    }
    let days = crate::civil::days_from_civil(y as i32, (m + 1) as u32, day as u32);
    days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + milli
}

/// `Date.parse(s)` — ISO 8601 string → ms-since-epoch as f64
/// (NaN on parse failure per ES §21.4.3.2). f64 ms values fit
/// exactly within the 52-bit mantissa for timestamps well past the
/// year 285616 (>= 2^53 ms beyond epoch); spec-correct NaN
/// replaces the prior `INT64_MIN` sentinel that fooled
/// `Number.isNaN` into returning false.
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_parse_iso(str_ptr: *const c_void) -> f64 {
    let ms = unsafe { parse_iso(str_ptr) };
    if ms == DATE_PARSE_FAIL {
        f64::NAN
    } else {
        ms as f64
    }
}

/// `new Date(iso)` — parse + allocate. Failure → epoch (best-effort,
/// matches C port; spec would yield NaN but tr's i64 has no NaN).
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_from_iso(str_ptr: *const c_void) -> *mut c_void {
    let mut ms = unsafe { parse_iso(str_ptr) };
    if ms == DATE_PARSE_FAIL {
        ms = 0;
    }
    alloc_date(ms)
}

// ---- Drop ----

/// # Safety
///
/// `d_ptr` is null or a Date pointer returned by one of the
/// allocators above.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_drop(d_ptr: *mut c_void) {
    if d_ptr.is_null() {
        return;
    }
    if unsafe { __torajs_rc_dec(d_ptr) } == 0 {
        return;
    }
    unsafe {
        let _ = Box::from_raw(d_ptr as *mut Date);
    }
}

// ---- Static ----

/// `Date.now()` — static. Returns ms since epoch (no heap alloc).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_now_static() -> i64 {
    now_ms()
}

// ---- Instance getters ----

/// `.getTime()` / `.valueOf()`.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_time(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    unsafe { as_date(d_ptr) }.ms
}

/// `.setTime(ms)` — overwrite in place, return new ms.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_time(d_ptr: *mut c_void, ms: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    unsafe {
        as_date_mut(d_ptr).ms = ms;
    }
    ms
}

/// Sentinel used by the per-field setter helpers (setFullYear /
/// setMonth / setHours / …) to mean "argument not provided — keep
/// the current value". `i64::MIN` is outside the JS Number safe
/// range, so it cannot collide with a real user-supplied value
/// after the ssa_lower `coerce_to_i64` truncation.
pub const DATE_FIELD_KEEP: i64 = i64::MIN;

/// Decompose `d.ms` into the 7-tuple (year-CE, JS-0-indexed-month,
/// day, hour, min, sec, milli) that the per-field setters need.
fn fetch_local_components(date: &Date) -> (i64, i64, i64, i64, i64, i64, i64) {
    let tm = localtime_decompose(date.ms);
    let milli = date.ms.rem_euclid(1000);
    (
        (tm.tm_year + 1900) as i64,
        tm.tm_mon as i64,
        tm.tm_mday as i64,
        tm.tm_hour as i64,
        tm.tm_min as i64,
        tm.tm_sec as i64,
        milli,
    )
}

#[inline]
fn pick(arg: i64, current: i64) -> i64 {
    if arg == DATE_FIELD_KEEP { current } else { arg }
}

/// `.setFullYear(year, month?, date?)` per ES §21.4.4.21 — overwrites
/// the LOCAL-time year (and optionally month/date), recomposes via
/// `components_to_local_ms`, returns the new `d.ms`.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_full_year(
    d_ptr: *mut c_void,
    year: i64,
    month: i64,
    day: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (_, m, d, h, mi, s, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(year, pick(month, m), pick(day, d), h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setMonth(month, date?)` per ES §21.4.4.25.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_month(d_ptr: *mut c_void, month: i64, day: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, _, d, h, mi, s, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(y, month, pick(day, d), h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setDate(date)` per ES §21.4.4.20.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_date(d_ptr: *mut c_void, day: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, _, h, mi, s, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(y, m, day, h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setHours(hour, min?, sec?, ms?)` per ES §21.4.4.22.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_hours(
    d_ptr: *mut c_void,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, _, mi, s, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(
        y,
        m,
        d,
        hour,
        pick(minute, mi),
        pick(second, s),
        pick(milli, ms),
    );
    date.ms = new_ms;
    new_ms
}

/// `.setMinutes(min, sec?, ms?)` per ES §21.4.4.24.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_minutes(
    d_ptr: *mut c_void,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, _, s, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(y, m, d, h, minute, pick(second, s), pick(milli, ms));
    date.ms = new_ms;
    new_ms
}

/// `.setSeconds(sec, ms?)` per ES §21.4.4.26.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_seconds(
    d_ptr: *mut c_void,
    second: i64,
    milli: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, mi, _, ms) = fetch_local_components(date);
    let new_ms = components_to_local_ms(y, m, d, h, mi, second, pick(milli, ms));
    date.ms = new_ms;
    new_ms
}

/// `.setMilliseconds(ms)` per ES §21.4.4.23.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_milliseconds(d_ptr: *mut c_void, milli: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, mi, s, _) = fetch_local_components(date);
    let new_ms = components_to_local_ms(y, m, d, h, mi, s, milli);
    date.ms = new_ms;
    new_ms
}

/// annexB `.getYear()` — year - 1900 in LOCAL time.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_year(d_ptr: *const c_void) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(ms);
    tm.tm_year as i64
}

/// annexB `.setYear(year)` — recompose with year applied (0-99 →
/// 1900-1999 per annexB rule).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_year(d_ptr: *mut c_void, year: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let year = if (0..100).contains(&year) {
        year + 1900
    } else {
        year
    };
    let cur_ms = unsafe { as_date(d_ptr) }.ms;
    let tm = localtime_decompose(cur_ms);
    let day_ms = 86_400_000i64;
    let days = cur_ms.div_euclid(day_ms);
    let mut tod = cur_ms - days * day_ms;
    if tod < 0 {
        tod += day_ms;
    }
    let new_ms = components_to_local_ms(
        year,
        tm.tm_mon as i64,
        tm.tm_mday as i64,
        tm.tm_hour as i64,
        tm.tm_min as i64,
        tm.tm_sec as i64,
        tod % 1000,
    );
    unsafe { as_date_mut(d_ptr).ms = new_ms };
    new_ms
}

// UTC + LOCAL per-field getters live in [`crate::getters`].

// ---- toISOString + toGMTString ----

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

const DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

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
