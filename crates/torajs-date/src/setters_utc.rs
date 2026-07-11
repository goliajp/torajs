//! Per-field UTC Date setters — `.setUTCFullYear` / `.setUTCMonth` /
//! `.setUTCDate` / `.setUTCHours` / `.setUTCMinutes` / `.setUTCSeconds`
//! / `.setUTCMilliseconds` per ES §21.4.4.
//!
//! Mirrors of the LOCAL setters in [`crate::api`] with the timezone
//! round-trip replaced by pure civil arithmetic: decompose via
//! [`crate::getters::decompose`], recompose via [`components_to_utc_ms`]
//! (spec MakeDay + MakeTime — no DST pass, and unlike `Date.UTC` /
//! the constructor, **no** two-digit-year 1900 mapping: MakeFullYear
//! does not apply to the setter family). Absent trailing arguments
//! arrive as [`crate::api::DATE_FIELD_KEEP`], same contract as the
//! LOCAL family.

use core::ffi::c_void;

use crate::api::DATE_FIELD_KEEP;
use crate::civil::days_from_civil;
use crate::getters::decompose;
use crate::{Date, as_date_mut};

/// Decompose `date.ms` into the UTC 7-tuple (year-CE, JS-0-indexed
/// month, day, hour, min, sec, milli) the per-field setters need.
fn fetch_utc_components(date: &Date) -> (i64, i64, i64, i64, i64, i64, i64) {
    let (y, m, d, h, mi, s, ms) = decompose(date.ms);
    (
        y as i64,
        m as i64 - 1, // civil 1..=12 → JS 0..=11
        d as i64,
        h as i64,
        mi as i64,
        s as i64,
        ms as i64,
    )
}

/// Recompose a UTC `(y, m0, d, h, min, sec, milli)` into ms since
/// epoch. Month overflow carries into years and day / time overflow
/// carries linearly from the first of the month, matching the LOCAL
/// recompose's normalization (spec MakeDay handles fractional /
/// out-of-range fields the same way).
fn components_to_utc_ms(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    let total_months = year * 12 + month;
    let y = total_months.div_euclid(12);
    let m0 = total_months.rem_euclid(12); // [0, 11]
    let base_days = days_from_civil(y as i32, (m0 + 1) as u32, 1);
    (base_days + day - 1) * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + milli
}

#[inline]
fn pick(arg: i64, current: i64) -> i64 {
    if arg == DATE_FIELD_KEEP { current } else { arg }
}

/// `.setUTCFullYear(year, month?, date?)` per ES §21.4.4.31.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_full_year(
    d_ptr: *mut c_void,
    year: i64,
    month: i64,
    day: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (_, m, d, h, mi, s, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(year, pick(month, m), pick(day, d), h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setUTCMonth(month, date?)` per ES §21.4.4.34.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_month(
    d_ptr: *mut c_void,
    month: i64,
    day: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, _, d, h, mi, s, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(y, month, pick(day, d), h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setUTCDate(date)` per ES §21.4.4.29.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_date(d_ptr: *mut c_void, day: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, _, h, mi, s, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(y, m, day, h, mi, s, ms);
    date.ms = new_ms;
    new_ms
}

/// `.setUTCHours(hour, min?, sec?, ms?)` per ES §21.4.4.32.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_hours(
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
    let (y, m, d, _, mi, s, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(
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

/// `.setUTCMinutes(min, sec?, ms?)` per ES §21.4.4.33.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_minutes(
    d_ptr: *mut c_void,
    minute: i64,
    second: i64,
    milli: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, _, s, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(y, m, d, h, minute, pick(second, s), pick(milli, ms));
    date.ms = new_ms;
    new_ms
}

/// `.setUTCSeconds(sec, ms?)` per ES §21.4.4.35.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_seconds(
    d_ptr: *mut c_void,
    second: i64,
    milli: i64,
) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, mi, _, ms) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(y, m, d, h, mi, second, pick(milli, ms));
    date.ms = new_ms;
    new_ms
}

/// `.setUTCMilliseconds(ms)` per ES §21.4.4.30.
///
/// # Safety
///
/// `d_ptr` is null or a live `*Date` (exclusive borrow).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_date_set_utc_milliseconds(d_ptr: *mut c_void, milli: i64) -> i64 {
    if d_ptr.is_null() {
        return 0;
    }
    let date = unsafe { as_date_mut(d_ptr) };
    let (y, m, d, h, mi, s, _) = fetch_utc_components(date);
    let new_ms = components_to_utc_ms(y, m, d, h, mi, s, milli);
    date.ms = new_ms;
    new_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::__torajs_date_from_ms;

    fn date_at(ms: i64) -> *mut c_void {
        // Leaked test cell — fine for a unit-test process.
        __torajs_date_from_ms(ms)
    }

    #[test]
    fn utc_recompose_round_trips() {
        // 2020-03-15T12:34:56.789Z
        let ms = 1_584_275_696_789i64;
        let (y, m, d, h, mi, s, milli) = decompose(ms);
        assert_eq!(
            components_to_utc_ms(
                y as i64,
                m as i64 - 1,
                d as i64,
                h as i64,
                mi as i64,
                s as i64,
                milli as i64
            ),
            ms
        );
    }

    #[test]
    fn set_utc_date_overwrites_day_only() {
        let ms = 1_584_275_696_789i64; // 2020-03-15T12:34:56.789Z
        let p = date_at(ms);
        let new_ms = unsafe { __torajs_date_set_utc_date(p, 1) };
        assert_eq!(new_ms, ms - 14 * 86_400_000);
    }

    #[test]
    fn set_utc_month_overflow_carries_year() {
        let ms = 1_584_275_696_789i64; // 2020-03-15
        let p = date_at(ms);
        unsafe { __torajs_date_set_utc_month(p, 12, DATE_FIELD_KEEP) };
        let (y, m, d, ..) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!((y, m, d), (2021, 1, 15));
    }

    #[test]
    fn set_utc_full_year_no_1900_mapping() {
        let p = date_at(0);
        unsafe { __torajs_date_set_utc_full_year(p, 50, DATE_FIELD_KEEP, DATE_FIELD_KEEP) };
        let (y, ..) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!(y, 50);
    }

    #[test]
    fn set_utc_hours_keeps_omitted_fields() {
        let ms = 1_584_275_696_789i64; // 12:34:56.789Z
        let p = date_at(ms);
        unsafe {
            __torajs_date_set_utc_hours(p, 5, DATE_FIELD_KEEP, DATE_FIELD_KEEP, DATE_FIELD_KEEP)
        };
        let (_, _, _, h, mi, s, milli) = decompose(unsafe { crate::as_date(p) }.ms);
        assert_eq!((h, mi, s, milli), (5, 34, 56, 789));
    }
}
