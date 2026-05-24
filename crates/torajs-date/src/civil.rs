//! Branch-free proleptic Gregorian arithmetic — port of
//! `runtime_date.c` L121-148 + L539-553.
//!
//! Two pure functions:
//!
//! - [`days_from_civil`] — `(y, m, d)` → days since 1970-01-01.
//!   Inverse of [`civil_from_days`].
//! - [`civil_from_days`] — `z` (days since 1970-01-01) → `(y, m, d)`.
//!   Handles negative `z` (pre-1970 dates) by the era trick.
//!
//! Algorithm: Howard Hinnant's `date_algorithms.html` (also the
//! basis of C++20 `<chrono>`'s civil-date arithmetic). Branch-free
//! integer division by 146097 (number of days in a 400-year era)
//! collapses the leap-year + century-year + 400-year cycle into
//! one O(1) formula.

/// Inverse of [`civil_from_days`]. `y` is the proleptic Gregorian
/// year; `m` is 1..=12; `d` is 1..=31. Returns days since
/// 1970-01-01 (signed; pre-1970 dates negative).
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u32; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

/// Given `z` = days since 1970-01-01, return `(year, month, day)`.
/// `month` is 1..=12; `day` is 1..=31.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u32; // [0, 146097)
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 400)
    let mut y = yoe as i32 + (era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_day_is_zero() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn one_day_before_epoch() {
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn round_trip_known_dates() {
        for (y, m, d) in [
            (2000, 1, 1),
            (2020, 2, 29), // leap day
            (1900, 3, 1),  // non-leap (century but not 400)
            (2024, 12, 31),
            (1, 1, 1),
            (9999, 12, 31),
        ] {
            let z = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(z), (y, m, d), "roundtrip {y}-{m}-{d}");
        }
    }

    #[test]
    fn known_y2k_day_count() {
        // 2000-01-01 = day 10957 from 1970-01-01.
        assert_eq!(days_from_civil(2000, 1, 1), 10957);
    }

    #[test]
    fn pre_1970_round_trip() {
        for y in [1900, 1500, 1000, 100, -1, -1000] {
            for (m, d) in [(1, 1), (6, 15), (12, 31)] {
                let z = days_from_civil(y, m, d);
                assert_eq!(civil_from_days(z), (y, m, d), "pre-1970 {y}-{m}-{d}");
            }
        }
    }
}
