//! Public extern "C" surface — ctors / getters / setters. Port of
//! `runtime_date.c` L77-589 (excluding civil + tm + parse helpers
//! extracted to siblings).
//!
//! RFC 20260713-date-invalid-time: time values and components cross
//! the ABI as f64 (spec number semantics — NaN = invalid date);
//! the in-memory `Date.ms` stays i64 with [`DATE_INVALID`] as the
//! NaN stand-in. Per-field setters take a trailing `present` bitmask
//! (bit k = argument k was supplied) — a supplied-but-NaN argument
//! invalidates the date, a missing one keeps the current field, so
//! the two can never be confused (the retired `DATE_FIELD_KEEP`
//! i64::MIN sentinel collided with NaN's saturating i64 cast).

use core::ffi::c_void;

use crate::make_time::{make_full_year, make_ms_local, ms_to_f64, time_clip};
use crate::parse::parse_iso;
use crate::tm::localtime_decompose;
use crate::{
    __torajs_rc_dec, DATE_INVALID, DATE_PARSE_FAIL, Date, HeapHeader, TAG_DATE, as_date,
    as_date_mut,
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

unsafe extern "C" {
    /// torajs-meta — scrub a dying exotic-subclass instance's
    /// identity entry (RFC 20260730 blade 0); gated on
    /// `FLAG_SUBCLASSED` in the drop below so plain dates never
    /// call out.
    fn __torajs_subclass_drop_entry(p: *mut c_void);
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
        // No own property has been written yet — the bag is minted
        // by the first `d.zz = 1`, never here.
        props: core::ptr::null_mut(),
    });
    Box::into_raw(d) as *mut c_void
}

/// Read the receiver's time value; `None` when the pointer is null
/// or the date is invalid. Shared by every getter / formatter.
pub(crate) fn valid_ms(d_ptr: *const c_void) -> Option<i64> {
    if d_ptr.is_null() {
        return None;
    }
    let ms = unsafe { as_date(d_ptr) }.ms;
    if ms == DATE_INVALID { None } else { Some(ms) }
}

/// `new Date()` — current wall clock.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_now() -> *mut c_void {
    alloc_date(now_ms())
}

/// `new Date(ms)` — from milliseconds since epoch, TimeClip'd
/// (NaN / |ms| > 8.64e15 → invalid date).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_from_ms(ms: f64) -> *mut c_void {
    alloc_date(time_clip(ms))
}

/// `new Date(y, m, d, h, mi, s, ms)` — LOCAL-time interpretation
/// with the MakeFullYear two-digit-year mapping (§21.4.2.1 step 6).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_from_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    milli: f64,
) -> *mut c_void {
    alloc_date(make_ms_local(
        make_full_year(year),
        month,
        day,
        hour,
        minute,
        second,
        milli,
    ))
}

/// `Date.UTC(y, m, d, h, mi, s, ms)` — pure UTC interpretation with
/// MakeFullYear; returns the clipped time value as a spec number
/// (NaN when out of range / any component NaN, §21.4.3.4).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_date_utc_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    milli: f64,
) -> f64 {
    ms_to_f64(crate::make_time::make_ms_utc(
        make_full_year(year),
        month,
        day,
        hour,
        minute,
        second,
        milli,
    ))
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

/// `new Date(iso)` — parse + allocate. Parse failure → invalid date
/// ([[DateValue]] = NaN per §21.4.2.1 step 5).
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_from_iso(str_ptr: *const c_void) -> *mut c_void {
    let mut ms = unsafe { parse_iso(str_ptr) };
    if ms == DATE_PARSE_FAIL {
        ms = DATE_INVALID;
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
        // Rotation 373 — a Date-subclass instance scrubs its
        // torajs-meta identity entry (RFC 20260730 blade 0); gated
        // on FLAG_SUBCLASSED so plain dates never call out.
        if (*(d_ptr as *const Date)).header.flags & crate::subclass::FLAG_SUBCLASSED != 0 {
            __torajs_subclass_drop_entry(d_ptr);
        }
        // Own-property bag (§21.4.4 ordinary-object face) — the
        // universal dispatcher routes it to the dynobj drop.
        let props = (*(d_ptr as *const Date)).props;
        if !props.is_null() {
            (*(d_ptr as *mut Date)).props = core::ptr::null_mut();
            crate::__torajs_value_drop_heap(props);
        }
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

/// `.getTime()` / `.valueOf()` — the time value (NaN when invalid).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_time(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => ms as f64,
        None => f64::NAN,
    }
}

/// `.setTime(ms)` — TimeClip + overwrite in place, return the new
/// time value (NaN when clipped to invalid).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_time(d_ptr: *mut c_void, ms: f64) -> f64 {
    if d_ptr.is_null() {
        return f64::NAN;
    }
    let new_ms = time_clip(ms);
    unsafe {
        as_date_mut(d_ptr).ms = new_ms;
    }
    ms_to_f64(new_ms)
}

/// Decompose `ms` into the 7-slot f64 component array
/// `[year-CE, JS-0-indexed-month, day, hour, min, sec, milli]`
/// (LOCAL time) that the per-field setter engine patches.
fn local_components(ms: i64) -> [f64; 7] {
    let tm = localtime_decompose(ms);
    let milli = ms.rem_euclid(1000);
    [
        (tm.tm_year + 1900) as f64,
        tm.tm_mon as f64,
        tm.tm_mday as f64,
        tm.tm_hour as f64,
        tm.tm_min as f64,
        tm.tm_sec as f64,
        milli as f64,
    ]
}

/// Shared per-field setter engine — LOCAL family (§21.4.4.20-27).
///
/// `start` = first patched slot in the 7-component array; `vals[k]`
/// carries slot `start + k`; `present` bit k = the argument was
/// supplied (a supplied NaN invalidates, a missing slot keeps the
/// current component). `nan_t_zero` = the spec's "if t is NaN, set
/// t to +0" step — the FullYear family only; every other setter on
/// an invalid date stays invalid and returns NaN.
pub(crate) unsafe fn set_fields_local(
    d_ptr: *mut c_void,
    start: usize,
    vals: &[f64],
    present: i64,
    nan_t_zero: bool,
) -> f64 {
    if d_ptr.is_null() {
        return f64::NAN;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    if date.ms == DATE_INVALID && !nan_t_zero {
        return f64::NAN;
    }
    // §21.4.4.21 step 4 — "If t is NaN, set t to +0𝔽; otherwise, set
    // t to LocalTime(t)": the invalid receiver's +0 is NOT shifted to
    // local time, so its components are the raw epoch tuple.
    let mut comps = if date.ms == DATE_INVALID {
        [1970.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]
    } else {
        local_components(date.ms)
    };
    for (k, v) in vals.iter().enumerate() {
        if present & (1 << k) != 0 {
            comps[start + k] = *v;
        }
    }
    let [y, mo, d, h, mi, s, milli] = comps;
    let new_ms = make_ms_local(y, mo, d, h, mi, s, milli);
    date.ms = new_ms;
    ms_to_f64(new_ms)
}

/// `.setFullYear(year, month?, date?)` per ES §21.4.4.21 — the one
/// LOCAL setter family that treats an invalid receiver as t = +0.
/// No MakeFullYear mapping (`setFullYear(50)` = year 50 CE).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_full_year(
    d_ptr: *mut c_void,
    year: f64,
    month: f64,
    day: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 0, &[year, month, day], present, true) }
}

/// `.setMonth(month, date?)` per ES §21.4.4.25.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_month(
    d_ptr: *mut c_void,
    month: f64,
    day: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 1, &[month, day], present, false) }
}

/// `.setDate(date)` per ES §21.4.4.20.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_date(d_ptr: *mut c_void, day: f64, present: i64) -> f64 {
    unsafe { set_fields_local(d_ptr, 2, &[day], present, false) }
}

/// `.setHours(hour, min?, sec?, ms?)` per ES §21.4.4.22.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_hours(
    d_ptr: *mut c_void,
    hour: f64,
    minute: f64,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 3, &[hour, minute, second, milli], present, false) }
}

/// `.setMinutes(min, sec?, ms?)` per ES §21.4.4.24.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_minutes(
    d_ptr: *mut c_void,
    minute: f64,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 4, &[minute, second, milli], present, false) }
}

/// `.setSeconds(sec, ms?)` per ES §21.4.4.26.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_seconds(
    d_ptr: *mut c_void,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 5, &[second, milli], present, false) }
}

/// `.setMilliseconds(ms)` per ES §21.4.4.23.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_milliseconds(
    d_ptr: *mut c_void,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_local(d_ptr, 6, &[milli], present, false) }
}

/// annexB `.getYear()` — year - 1900 in LOCAL time (NaN when
/// invalid).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_get_year(d_ptr: *const c_void) -> f64 {
    match valid_ms(d_ptr) {
        Some(ms) => localtime_decompose(ms).tm_year as f64,
        None => f64::NAN,
    }
}

/// annexB `.setYear(year)` per §B.2.4.2 — invalid receiver treated
/// as t = +0; NaN year invalidates; 0-99 maps to 1900-1999
/// (MakeFullYear).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_year(d_ptr: *mut c_void, year: f64) -> f64 {
    unsafe { set_fields_local(d_ptr, 0, &[make_full_year(year)], 1, true) }
}
