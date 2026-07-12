//! Spec time-value math over f64 components — MakeDay / MakeTime /
//! MakeDate / MakeFullYear / TimeClip (ES §21.4.1.27-32).
//!
//! RFC 20260713-date-invalid-time: the Date ABI carries time values
//! and components as f64 (spec number semantics — NaN = invalid),
//! while the in-memory `Date.ms` stays i64 with [`crate::DATE_INVALID`]
//! (`i64::MIN`) standing in for the NaN time value (valid range
//! |t| ≤ 8.64e15 can never collide).
//!
//! Composition runs in f64 exactly like the spec's 𝔽 arithmetic, so
//! overflowing components propagate to ±inf/NaN and land on
//! [`DATE_INVALID`] via [`time_clip`] — no i64 intermediate overflow.

use crate::DATE_INVALID;
use crate::civil::days_from_civil;
use crate::tz::local_utoff;

/// Spec TimeClip bound — ±8.64e15 ms (±100_000_000 days).
pub const MAX_TIME_MS_F: f64 = 8.64e15;

/// TimeClip (§21.4.1.31): non-finite or |t| > 8.64e15 → invalid
/// (spec NaN); otherwise truncate toward zero.
pub fn time_clip(t: f64) -> i64 {
    if !t.is_finite() || t.abs() > MAX_TIME_MS_F {
        DATE_INVALID
    } else {
        t.trunc() as i64
    }
}

/// Internal time value → spec number: invalid sentinel → NaN.
pub fn ms_to_f64(ms: i64) -> f64 {
    if ms == DATE_INVALID {
        f64::NAN
    } else {
        ms as f64
    }
}

/// MakeFullYear (§21.4.1.32) two-digit-year mapping: truncated
/// 0..=99 → 1900+. NaN passes through untouched. Applies to the
/// `new Date(y, …)` ctor, `Date.UTC`, and annexB `setYear` — NOT
/// to the `setFullYear` family.
pub fn make_full_year(y: f64) -> f64 {
    let yi = y.trunc();
    if (0.0..=99.0).contains(&yi) {
        1900.0 + yi
    } else {
        y
    }
}

/// MakeDay + MakeTime + MakeDate over already-ToNumber'd f64
/// components → f64 ms (pure UTC, no zone pass, no MakeFullYear).
/// Any non-finite component or a year outside the calendar range
/// yields NaN, which the caller's [`time_clip`] maps to invalid.
pub fn make_date_ms(y: f64, mo: f64, d: f64, h: f64, mi: f64, s: f64, milli: f64) -> f64 {
    for v in [y, mo, d, h, mi, s, milli] {
        if !v.is_finite() {
            return f64::NAN;
        }
    }
    let (y, mo, d) = (y.trunc(), mo.trunc(), d.trunc());
    let (h, mi, s, milli) = (h.trunc(), mi.trunc(), s.trunc(), milli.trunc());
    // month overflow carries into years (f64 floor-div is exact while
    // |y| stays in the calendar gate below)
    let ym = y + (mo / 12.0).floor();
    // |valid years| ≤ 275_760; anything past 300k is unconditionally
    // out of TimeClip range, so gate before the i32 civil math
    if ym.abs() > 300_000.0 {
        return f64::NAN;
    }
    let m0 = mo.rem_euclid(12.0) as u32; // [0, 11]
    let base_days = days_from_civil(ym as i32, m0 + 1, 1) as f64;
    (base_days + d - 1.0) * 86_400_000.0 + h * 3_600_000.0 + mi * 60_000.0 + s * 1000.0 + milli
}

/// UTC composition → clipped internal time value.
pub fn make_ms_utc(y: f64, mo: f64, d: f64, h: f64, mi: f64, s: f64, milli: f64) -> i64 {
    time_clip(make_date_ms(y, mo, d, h, mi, s, milli))
}

/// LOCAL composition — compose in f64, then UTC(t) via the two-pass
/// DST-aware offset (textbook mktime with `tm_isdst = -1`), then
/// TimeClip. The pre-offset gate keeps the i64 conversion for the
/// zone lookup in range (offsets are ≤ ±14 h; 48 h of slack).
pub fn make_ms_local(y: f64, mo: f64, d: f64, h: f64, mi: f64, s: f64, milli: f64) -> i64 {
    let local = make_date_ms(y, mo, d, h, mi, s, milli);
    if !local.is_finite() || local.abs() > MAX_TIME_MS_F + 48.0 * 3_600_000.0 {
        return DATE_INVALID;
    }
    let local_secs = (local as i64).div_euclid(1000);
    let off1 = local_utoff(local_secs) as i64;
    let off2 = local_utoff(local_secs - off1) as i64;
    time_clip(local - (off2 * 1000) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_clip_bounds() {
        assert_eq!(time_clip(f64::NAN), DATE_INVALID);
        assert_eq!(time_clip(f64::INFINITY), DATE_INVALID);
        assert_eq!(time_clip(8.64e15 + 1.0), DATE_INVALID);
        assert_eq!(time_clip(8.64e15), 8_640_000_000_000_000);
        assert_eq!(time_clip(-8.64e15), -8_640_000_000_000_000);
        assert_eq!(time_clip(-1.5), -1);
        assert_eq!(time_clip(0.0), 0);
    }

    #[test]
    fn ms_to_f64_invalid_is_nan() {
        assert!(ms_to_f64(DATE_INVALID).is_nan());
        assert_eq!(ms_to_f64(0), 0.0);
    }

    #[test]
    fn make_full_year_maps_two_digit() {
        assert_eq!(make_full_year(50.0), 1950.0);
        assert_eq!(make_full_year(0.0), 1900.0);
        assert_eq!(make_full_year(99.9), 1999.0);
        assert_eq!(make_full_year(100.0), 100.0);
        assert_eq!(make_full_year(-1.0), -1.0);
        assert!(make_full_year(f64::NAN).is_nan());
    }

    #[test]
    fn make_date_ms_epoch() {
        assert_eq!(make_date_ms(1970.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0), 0.0);
    }

    #[test]
    fn make_date_ms_nan_component() {
        assert!(make_date_ms(f64::NAN, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0).is_nan());
        assert!(make_date_ms(2020.0, 0.0, 1.0, 0.0, 0.0, f64::INFINITY, 0.0).is_nan());
    }

    #[test]
    fn make_date_ms_month_carry() {
        // 2020-13 → 2021-02
        let a = make_date_ms(2020.0, 13.0, 1.0, 0.0, 0.0, 0.0, 0.0);
        let b = make_date_ms(2021.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(a, b);
    }

    #[test]
    fn make_ms_utc_clip_overflow() {
        // +275760-09-13 is the max representable day; one past → invalid
        assert_eq!(
            make_ms_utc(275_760.0, 8.0, 13.0, 0.0, 0.0, 0.0, 0.0),
            8_640_000_000_000_000
        );
        assert_eq!(
            make_ms_utc(275_760.0, 8.0, 14.0, 0.0, 0.0, 0.0, 0.0),
            DATE_INVALID
        );
    }
}
