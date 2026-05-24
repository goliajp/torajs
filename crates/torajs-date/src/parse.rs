//! ISO 8601 parser — port of `runtime_date.c` L164-265.
//!
//! Accepts the canonical extended format
//! `YYYY-MM-DDTHH:MM:SS.sssZ` and several relaxations:
//! - date-only (`YYYY-MM-DD`) → midnight UTC
//! - extended year sign (`+YYYYYY` / `-YYYYYY`, 6-digit year)
//! - timezone (`Z` / `+HH:MM` / `-HH:MM`)
//! - space separator (`YYYY-MM-DD HH:MM:SS`) in addition to `T`
//!
//! Returns ms-since-epoch on success, [`crate::DATE_PARSE_FAIL`]
//! (== `i64::MIN`) on any malformed input.

use core::ffi::c_void;

use crate::tm::components_to_local_ms;
use crate::{DATE_PARSE_FAIL, STR_HDR_SIZE};

/// Read exactly `n_digits` ASCII digits at `*i`. On success,
/// advances `*i` past them and returns `Some(value)`. On any
/// short-read / non-digit input, returns `None` without advancing.
fn read_int(s: &[u8], i: &mut usize, n_digits: usize) -> Option<i64> {
    if *i + n_digits > s.len() {
        return None;
    }
    let mut v: i64 = 0;
    for k in 0..n_digits {
        let c = s[*i + k];
        if !c.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (c - b'0') as i64;
    }
    *i += n_digits;
    Some(v)
}

/// Days-since-epoch from `(year, mon-1, day, h, min, sec, ms)` as
/// pure UTC. Mirrors `__torajs_date_utc_components` in the C port.
fn utc_components_to_ms(
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
    // Normalize month overflow into year.
    let mut y = year + month.div_euclid(12);
    let m = month.rem_euclid(12);
    if m < 0 {
        // div_euclid + rem_euclid keep m >= 0 already; guard for clarity.
        y -= 1;
    }
    let days = crate::civil::days_from_civil(y as i32, (m + 1) as u32, day as u32);
    days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + milli
}

/// Parse `str_ptr` (a tora `*Str`) as an ISO 8601 timestamp.
/// Returns ms since epoch on success, [`DATE_PARSE_FAIL`] on
/// failure.
///
/// # Safety
///
/// `str_ptr` is null or a live `*Str`.
pub unsafe fn parse_iso(str_ptr: *const c_void) -> i64 {
    if str_ptr.is_null() {
        return DATE_PARSE_FAIL;
    }
    let len = unsafe { *((str_ptr as *const u8).add(8) as *const u64) } as usize;
    let s = unsafe { core::slice::from_raw_parts((str_ptr as *const u8).add(STR_HDR_SIZE), len) };
    parse_iso_bytes(s).unwrap_or(DATE_PARSE_FAIL)
}

fn parse_iso_bytes(s: &[u8]) -> Option<i64> {
    let mut i = 0;

    // Optional leading sign for extended-year form.
    let year = parse_year(s, &mut i)?;

    let mut mon: i64 = 1;
    let mut day: i64 = 1;
    let mut hour: i64 = 0;
    let mut minute: i64 = 0;
    let mut second: i64 = 0;
    let mut milli: i64 = 0;
    let mut has_time = false;
    let mut has_z = false;
    let mut tz_sign: i64 = 0;
    let mut tz_h: i64 = 0;
    let mut tz_m: i64 = 0;

    if let Some(&b'-') = s.get(i) {
        i += 1;
        mon = read_int(s, &mut i, 2)?;
        if let Some(&b'-') = s.get(i) {
            i += 1;
            day = read_int(s, &mut i, 2)?;
        }
    }
    if let Some(&c) = s.get(i)
        && (c == b'T' || c == b' ')
    {
        i += 1;
        has_time = true;
        hour = read_int(s, &mut i, 2)?;
        if let Some(&b':') = s.get(i) {
            i += 1;
            minute = read_int(s, &mut i, 2)?;
            if let Some(&b':') = s.get(i) {
                i += 1;
                second = read_int(s, &mut i, 2)?;
                if let Some(&b'.') = s.get(i) {
                    i += 1;
                    let mut digits = 0;
                    while digits < 3 && i < s.len() && s[i].is_ascii_digit() {
                        milli = milli * 10 + (s[i] - b'0') as i64;
                        i += 1;
                        digits += 1;
                    }
                    while digits < 3 {
                        milli *= 10;
                        digits += 1;
                    }
                    // Skip remaining sub-ms digits.
                    while i < s.len() && s[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
        }
        if let Some(&c) = s.get(i) {
            if c == b'Z' {
                has_z = true;
                i += 1;
            } else if c == b'+' || c == b'-' {
                tz_sign = if c == b'+' { 1 } else { -1 };
                i += 1;
                tz_h = read_int(s, &mut i, 2)?;
                if let Some(&b':') = s.get(i) {
                    i += 1;
                }
                tz_m = read_int(s, &mut i, 2).unwrap_or(0);
            }
        }
    }
    if i != s.len() {
        return None;
    }
    let ms = if !has_time {
        // Date-only ISO → UTC midnight.
        utc_components_to_ms(year, mon - 1, day, 0, 0, 0, 0)
    } else if has_z {
        utc_components_to_ms(year, mon - 1, day, hour, minute, second, milli)
    } else if tz_sign != 0 {
        let base = utc_components_to_ms(year, mon - 1, day, hour, minute, second, milli);
        let off_ms = (tz_h * 60 + tz_m) * 60 * 1000 * tz_sign;
        base - off_ms
    } else {
        components_to_local_ms(year, mon - 1, day, hour, minute, second, milli)
    };
    Some(ms)
}

fn parse_year(s: &[u8], i: &mut usize) -> Option<i64> {
    let sign = match s.first() {
        Some(&b'+') => {
            *i += 1;
            1
        }
        Some(&b'-') => {
            *i += 1;
            -1
        }
        _ => 0,
    };
    let y = if sign != 0 {
        // Extended-year form = 6 digits.
        read_int(s, i, 6)?
    } else {
        read_int(s, i, 4)?
    };
    Some(if sign < 0 { -y } else { y })
}

#[cfg(test)]
mod tests {
    use super::*;

    // To unit-test parse_iso we'd need a `*Str` allocator stub —
    // exercise via integration tests at the workspace level
    // instead. Here just sanity-check the building blocks.

    #[test]
    fn read_int_advances_on_success() {
        let s = b"2024-06-15";
        let mut i = 0;
        assert_eq!(read_int(s, &mut i, 4), Some(2024));
        assert_eq!(i, 4);
    }

    #[test]
    fn read_int_fails_on_non_digit() {
        let s = b"2x24";
        let mut i = 0;
        assert_eq!(read_int(s, &mut i, 4), None);
    }

    #[test]
    fn utc_components_epoch_zero() {
        assert_eq!(utc_components_to_ms(1970, 0, 1, 0, 0, 0, 0), 0);
    }

    #[test]
    fn utc_components_one_day_after_epoch() {
        assert_eq!(utc_components_to_ms(1970, 0, 2, 0, 0, 0, 0), 86_400_000);
    }

    #[test]
    fn utc_components_y2k() {
        // 2000-01-01T00:00:00Z = 946684800 secs * 1000.
        assert_eq!(
            utc_components_to_ms(2000, 0, 1, 0, 0, 0, 0),
            946_684_800_000
        );
    }
}
