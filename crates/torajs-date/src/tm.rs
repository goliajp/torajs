//! POSIX `struct tm` mirror + timezone-aware conversions.
//!
//! Originally a port of `runtime_date.c` backed by libc `localtime_r`
//! / `mktime`. Those two libc fns were the last libc holdout on the
//! Date path; they are now reimplemented in-house on [`crate::tz`]
//! (an `/etc/localtime` TZif reader, offset-only) plus the pure-Rust
//! `civil` calendar math — no libc, no FFI.
//!
//! - [`localtime_decompose`] — UTC ms → local-zone `Tm` (was
//!   `localtime_r`): shift the UTC instant by `tz::local_utoff`, then
//!   `civil_from_days` + time-of-day split.
//! - [`components_to_local_ms`] — local `(y,m,d,h,min,s)` → UTC ms
//!   (was `mktime`): `days_from_civil` to a local instant, then a
//!   two-pass DST-aware offset subtraction (textbook `tm_isdst = -1`).
//!
//! The `Tm` struct keeps its POSIX shape + extension padding so the
//! getter call-sites and the round-trip tests are unchanged.

use crate::civil::{civil_from_days, days_from_civil};
use crate::tz::local_utoff;

/// POSIX `struct tm`. Field order matches POSIX; the trailing padding
/// historically covered glibc / macOS extension fields (`tm_gmtoff`,
/// `tm_zone`) written by libc — retained so the layout and the getter
/// return shape stay identical after the in-house cutover.
#[repr(C)]
#[derive(Default, Clone, Copy)]
pub struct Tm {
    pub tm_sec: i32,
    pub tm_min: i32,
    pub tm_hour: i32,
    pub tm_mday: i32,
    pub tm_mon: i32,
    pub tm_year: i32,
    pub tm_wday: i32,
    pub tm_yday: i32,
    pub tm_isdst: i32,
    /// Extension-field reserve — glibc/macOS write `tm_gmtoff`
    /// (long) + `tm_zone` (char*) past offset 36. 24 B covers
    /// 8-byte alignment + 16-byte payload comfortably.
    pub _pad: [u8; 24],
}

/// Decompose `ms` (UNIX ms) into LOCAL-zone `Tm`. Sub-second
/// (`ms % 1000`) stays accessible separately.
pub fn localtime_decompose(ms: i64) -> Tm {
    let utc_secs = ms.div_euclid(1000);
    let local_secs = utc_secs + local_utoff(utc_secs) as i64;
    let days = local_secs.div_euclid(86400);
    let tod = local_secs.rem_euclid(86400); // [0, 86400)
    let (y, m, d) = civil_from_days(days);
    Tm {
        tm_sec: (tod % 60) as i32,
        tm_min: ((tod / 60) % 60) as i32,
        tm_hour: (tod / 3600) as i32,
        tm_mday: d as i32,
        tm_mon: (m - 1) as i32, // civil 1..=12 → POSIX tm_mon 0..=11
        tm_year: y - 1900,
        // 1970-01-01 was a Thursday (4); JS getDay() / tm_wday are both
        // 0 = Sunday. rem_euclid keeps it correct for pre-epoch days.
        tm_wday: ((days.rem_euclid(7) + 4) % 7) as i32,
        tm_yday: 0, // no getter exposes day-of-year
        tm_isdst: 0,
        _pad: [0; 24],
    }
}

/// Recompose `(y, m, d, h, min, sec)` (LOCAL-time interpretation
/// per JS spec) plus `milli` sub-second into `ms` since UNIX epoch.
///
/// JS quirk: `0 ≤ year < 100` is interpreted as `1900-1999` (legacy
/// behavior preserved by every browser); enforce it here.
pub fn components_to_local_ms(
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
    // Normalize JS 0-indexed month overflow into years (new Date(2020,
    // 13, …) → 2021). div_euclid handles negative months too.
    let total_months = year * 12 + month;
    let y = total_months.div_euclid(12);
    let m0 = total_months.rem_euclid(12); // [0, 11]
    // Build the local instant from the first of that month, then carry
    // day / hour / minute / second as raw seconds — this normalizes any
    // overflow (day=32, hour=25, …) exactly the way libc mktime did.
    let base_days = days_from_civil(y as i32, (m0 + 1) as u32, 1);
    let local_secs = base_days * 86400 + (day - 1) * 86400 + hour * 3600 + minute * 60 + second;
    // Two-pass DST resolution (textbook mktime with tm_isdst = -1):
    // guess the offset treating the local instant as UTC, then re-query
    // at the corrected instant so spring-forward gaps / fall-back
    // overlaps resolve like the libc implementation did.
    let off1 = local_utoff(local_secs) as i64;
    let off2 = local_utoff(local_secs - off1) as i64;
    let utc_secs = local_secs - off2;
    utc_secs * 1000 + milli
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tm_struct_at_least_36_bytes() {
        // 9 i32 fields × 4 = 36; plus padding for extension fields.
        assert!(core::mem::size_of::<Tm>() >= 36);
    }

    #[test]
    fn localtime_round_trip_smoke() {
        // 2024-06-15T12:00:00Z = 1718452800 secs = 1718452800000 ms.
        let tm = localtime_decompose(1_718_452_800_000);
        // Year + month + day shouldn't be zero (would mean localtime_r
        // failed). We don't assert exact values because the host's TZ
        // shifts them, but year ≥ 124 (= 2024 - 1900) is a sanity floor.
        assert!(tm.tm_year >= 120, "tm_year={}", tm.tm_year);
        assert!(tm.tm_year <= 130);
    }

    #[test]
    fn component_round_trip() {
        // local components → ms → decompose must restore the same local
        // wall clock: the offset applied by components_to_local_ms and
        // removed by localtime_decompose cancel regardless of the host
        // zone / DST (June 15 is never a DST-transition day, so 12:30 is
        // an unambiguous local instant).
        let ms = components_to_local_ms(2024, 5, 15, 12, 30, 45, 0); // month 5 = June (JS 0-indexed)
        let tm = localtime_decompose(ms);
        assert_eq!(tm.tm_year + 1900, 2024);
        assert_eq!(tm.tm_mon, 5);
        assert_eq!(tm.tm_mday, 15);
        assert_eq!(tm.tm_hour, 12);
        assert_eq!(tm.tm_min, 30);
        assert_eq!(tm.tm_sec, 45);
    }

    #[test]
    fn month_overflow_normalizes() {
        // new Date(2020, 13, 1) → 2021-02-01, the libc mktime behavior.
        let ms = components_to_local_ms(2020, 13, 1, 0, 0, 0, 0);
        let tm = localtime_decompose(ms);
        assert_eq!(tm.tm_year + 1900, 2021);
        assert_eq!(tm.tm_mon, 1); // February (0-indexed)
        assert_eq!(tm.tm_mday, 1);
    }
}
