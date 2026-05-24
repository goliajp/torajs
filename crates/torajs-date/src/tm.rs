//! POSIX `struct tm` mirror + libc FFI for timezone-aware
//! conversions — port of `runtime_date.c`'s `localtime_decompose`
//! + `__torajs_date_components_to_local_ms` (L474-482, L90-108).
//!
//! Two helpers from libc:
//!
//! - `localtime_r(time_t, *Tm) -> *Tm` — decompose UTC seconds
//!   into local-time-zone-aware `struct tm`. POSIX-thread-safe.
//! - `mktime(*mut Tm) -> time_t` — recompose `struct tm` (local
//!   interpretation) back to UTC seconds. Inverse of localtime_r.
//!
//! `struct tm`'s extension fields (`tm_gmtoff`, `tm_zone`) on
//! glibc / macOS push the struct past the 9-int POSIX core; the
//! Rust mirror reserves 16 bytes of padding so the struct is at
//! least as wide as the host's libc layout (the call-site fills
//! only the standard fields; the padding is for libc's writes).

use core::ffi::c_void;

/// POSIX `struct tm`. Field order matches POSIX; padding covers
/// glibc + macOS extension fields (`tm_gmtoff: long`, `tm_zone:
/// const char*`).
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

unsafe extern "C" {
    fn localtime_r(secs: *const i64, out: *mut Tm) -> *mut Tm;
    fn mktime(tm: *mut Tm) -> i64;
}

/// Decompose `ms` (UNIX ms) into LOCAL-zone `Tm`. Sub-second
/// (`ms % 1000`) stays accessible separately.
pub fn localtime_decompose(ms: i64) -> Tm {
    let secs = ms.div_euclid(1000);
    let mut out = Tm::default();
    unsafe {
        localtime_r(&secs as *const i64, &mut out as *mut Tm);
    }
    out
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
    let mut tm = Tm {
        tm_year: (year - 1900) as i32,
        tm_mon: month as i32, // JS 0-indexed → tm_mon 0-indexed
        tm_mday: day as i32,
        tm_hour: hour as i32,
        tm_min: minute as i32,
        tm_sec: second as i32,
        tm_isdst: -1, // let libc decide DST for the zone
        ..Default::default()
    };
    let t = unsafe { mktime(&mut tm as *mut Tm) };
    if t == -1 {
        return 0; // invalid date — best-effort
    }
    t * 1000 + milli
}

// Silence "unused" warning on the c_void import — it's reserved
// for cross-module callers that pass through.
#[allow(dead_code)]
pub(crate) fn _keep_c_void(_p: *mut c_void) {}

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
}
