//! Public extern "C" surface — ctors / getters / setters /
//! toISOString / toGMTString. Port of `runtime_date.c` L77-589
//! (excluding civil + tm + parse helpers extracted to siblings).

use core::ffi::c_void;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::civil::civil_from_days;
use crate::getters::decompose;
use crate::parse::parse_iso;
use crate::tm::{components_to_local_ms, localtime_decompose};
use crate::{
    __torajs_rc_dec, __torajs_str_alloc_pooled, DATE_PARSE_FAIL, Date, HeapHeader, STR_HDR_SIZE,
    TAG_DATE, as_date, as_date_mut,
};

// ---- Time source ----

/// Wall-clock ms since UNIX epoch via std SystemTime. Returns 0
/// on the (extremely rare) clock-skew-pre-epoch case.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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

/// `Date.parse(s)` — ISO 8601 string → ms (or [`DATE_PARSE_FAIL`]).
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_parse_iso(str_ptr: *const c_void) -> i64 {
    unsafe { parse_iso(str_ptr) }
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
