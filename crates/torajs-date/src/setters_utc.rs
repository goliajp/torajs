//! Per-field UTC Date setters — `.setUTCFullYear` / `.setUTCMonth` /
//! `.setUTCDate` / `.setUTCHours` / `.setUTCMinutes` / `.setUTCSeconds`
//! / `.setUTCMilliseconds` per ES §21.4.4.29-35.
//!
//! Mirrors of the LOCAL setters in [`crate::api`] with the timezone
//! round-trip replaced by pure civil arithmetic: decompose via
//! [`crate::getters::decompose`], recompose via
//! [`crate::make_time::make_ms_utc`] (spec MakeDay + MakeTime — no
//! DST pass, and unlike `Date.UTC` / the constructor, **no**
//! two-digit-year 1900 mapping: MakeFullYear does not apply to the
//! setter family). Same trailing `present` bitmask contract as the
//! LOCAL family (RFC 20260713-date-invalid-time): a supplied NaN
//! invalidates, a missing slot keeps the current component.

use core::ffi::c_void;

use crate::getters::decompose;
use crate::make_time::{make_ms_utc, ms_to_f64};
use crate::{DATE_INVALID, as_date_mut};

/// Decompose `ms` into the 7-slot f64 component array
/// `[year-CE, JS-0-indexed-month, day, hour, min, sec, milli]`
/// (pure UTC) that the setter engine patches.
fn utc_components(ms: i64) -> [f64; 7] {
    let (y, m, d, h, mi, s, milli) = decompose(ms);
    [
        y as f64,
        (m - 1) as f64, // civil 1..=12 → JS 0..=11
        d as f64,
        h as f64,
        mi as f64,
        s as f64,
        milli as f64,
    ]
}

/// Shared per-field setter engine — UTC family. Same contract as
/// [`crate::api::set_fields_local`] (`start` / `vals` / `present` /
/// `nan_t_zero`), recomposed without the zone pass.
unsafe fn set_fields_utc(
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
    let base = if date.ms == DATE_INVALID { 0 } else { date.ms };
    let mut comps = utc_components(base);
    for (k, v) in vals.iter().enumerate() {
        if present & (1 << k) != 0 {
            comps[start + k] = *v;
        }
    }
    let [y, mo, d, h, mi, s, milli] = comps;
    let new_ms = make_ms_utc(y, mo, d, h, mi, s, milli);
    date.ms = new_ms;
    ms_to_f64(new_ms)
}

/// `.setUTCFullYear(year, month?, date?)` per ES §21.4.4.31 —
/// invalid receiver treated as t = +0 (FullYear family only).
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_full_year(
    d_ptr: *mut c_void,
    year: f64,
    month: f64,
    day: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 0, &[year, month, day], present, true) }
}

/// `.setUTCMonth(month, date?)` per ES §21.4.4.34.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_month(
    d_ptr: *mut c_void,
    month: f64,
    day: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 1, &[month, day], present, false) }
}

/// `.setUTCDate(date)` per ES §21.4.4.29.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_date(
    d_ptr: *mut c_void,
    day: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 2, &[day], present, false) }
}

/// `.setUTCHours(hour, min?, sec?, ms?)` per ES §21.4.4.32.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_hours(
    d_ptr: *mut c_void,
    hour: f64,
    minute: f64,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 3, &[hour, minute, second, milli], present, false) }
}

/// `.setUTCMinutes(min, sec?, ms?)` per ES §21.4.4.33.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_minutes(
    d_ptr: *mut c_void,
    minute: f64,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 4, &[minute, second, milli], present, false) }
}

/// `.setUTCSeconds(sec, ms?)` per ES §21.4.4.35.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_seconds(
    d_ptr: *mut c_void,
    second: f64,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 5, &[second, milli], present, false) }
}

/// `.setUTCMilliseconds(ms)` per ES §21.4.4.30.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_milliseconds(
    d_ptr: *mut c_void,
    milli: f64,
    present: i64,
) -> f64 {
    unsafe { set_fields_utc(d_ptr, 6, &[milli], present, false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::__torajs_date_from_ms;

    fn date_at(ms: i64) -> *mut c_void {
        // Leaked test cell — fine for a unit-test process.
        __torajs_date_from_ms(ms as f64)
    }

    #[test]
    fn utc_recompose_round_trips() {
        // 2020-03-15T12:34:56.789Z
        let ms = 1_584_275_696_789i64;
        let [y, m, d, h, mi, s, milli] = utc_components(ms);
        assert_eq!(make_ms_utc(y, m, d, h, mi, s, milli), ms);
    }

    #[test]
    fn set_utc_date_overwrites_day_only() {
        let ms = 1_584_275_696_789i64; // 2020-03-15T12:34:56.789Z
        let p = date_at(ms);
        let new_ms = unsafe { __torajs_date_set_utc_date(p, 1.0, 0b1) };
        assert_eq!(new_ms, (ms - 14 * 86_400_000) as f64);
    }

    #[test]
    fn set_utc_month_overflow_carries_year() {
        let ms = 1_584_275_696_789i64; // 2020-03-15
        let p = date_at(ms);
        unsafe { __torajs_date_set_utc_month(p, 12.0, 0.0, 0b01) };
        let (y, m, d, ..) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!((y, m, d), (2021, 1, 15));
    }

    #[test]
    fn set_utc_full_year_no_1900_mapping() {
        let p = date_at(0);
        unsafe { __torajs_date_set_utc_full_year(p, 50.0, 0.0, 0.0, 0b001) };
        let (y, ..) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!(y, 50);
    }

    #[test]
    fn set_utc_hours_keeps_omitted_fields() {
        let ms = 1_584_275_696_789i64; // 12:34:56.789Z
        let p = date_at(ms);
        unsafe { __torajs_date_set_utc_hours(p, 5.0, 0.0, 0.0, 0.0, 0b0001) };
        let (_, _, _, h, mi, s, milli) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!((h, mi, s, milli), (5, 34, 56, 789));
    }

    #[test]
    fn set_utc_hours_nan_invalidates() {
        let p = date_at(0);
        let r = unsafe { __torajs_date_set_utc_hours(p, f64::NAN, 0.0, 0.0, 0.0, 0b0001) };
        assert!(r.is_nan());
        assert_eq!(unsafe { crate::as_date(p) }.ms, DATE_INVALID);
    }

    #[test]
    fn setter_on_invalid_receiver_stays_invalid() {
        let p = date_at(0);
        unsafe { __torajs_date_set_utc_hours(p, f64::NAN, 0.0, 0.0, 0.0, 0b0001) };
        let r = unsafe { __torajs_date_set_utc_date(p, 5.0, 0b1) };
        assert!(r.is_nan());
        assert_eq!(unsafe { crate::as_date(p) }.ms, DATE_INVALID);
    }

    #[test]
    fn set_utc_full_year_revives_invalid_receiver() {
        let p = date_at(0);
        unsafe { __torajs_date_set_utc_hours(p, f64::NAN, 0.0, 0.0, 0.0, 0b0001) };
        let r = unsafe { __torajs_date_set_utc_full_year(p, 2020.0, 0.0, 0.0, 0b001) };
        assert!(!r.is_nan());
        let (y, m, d, ..) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!((y, m, d), (2020, 1, 1));
    }

    #[test]
    fn set_utc_date_time_clip_overflow_invalidates() {
        // receiver at the max time value; pushing the day past the
        // boundary must TimeClip to invalid and return NaN
        let p = date_at(8_640_000_000_000_000);
        let r = unsafe { __torajs_date_set_utc_date(p, 28.0, 0b1) };
        assert!(r.is_nan());
        assert_eq!(unsafe { crate::as_date(p) }.ms, DATE_INVALID);
    }
}
