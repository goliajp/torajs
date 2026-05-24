//! Math namespace intrinsics — `Math.sqrt(x)` / `Math.abs(x)` /
//! `Math.pow(x, y)` / etc.
//!
//! Each extern fn is a thin wrapper over Rust's `f64::X(self)` /
//! `f64::X(self, other)` methods. Rust stdlib delegates to libm at
//! the same call site the IR-emitted versions used (the
//! `define_math_unary` / `define_math_binary` builders in
//! `ssa_inkwell` emitted single libm calls; both helpers + their
//! 27 dispatch arms deleted at P3.2-b ship 2026-05-23).
//!
//! ## libm-equivalent symbol map
//!
//! | torajs fn                | Rust f64 method | libm symbol |
//! |---|---|---|
//! | `__torajs_math_sqrt`     | `x.sqrt()`      | `sqrt`      |
//! | `__torajs_math_abs`      | `x.abs()`       | `fabs`      |
//! | `__torajs_math_floor`    | `x.floor()`     | `floor`     |
//! | `__torajs_math_ceil`     | `x.ceil()`      | `ceil`      |
//! | `__torajs_math_round`    | `x.round()`     | `round`     |
//! | `__torajs_math_trunc`    | `x.trunc()`     | `trunc`     |
//! | `__torajs_math_cbrt`     | `x.cbrt()`      | `cbrt`      |
//! | `__torajs_math_exp`      | `x.exp()`       | `exp`       |
//! | `__torajs_math_expm1`    | `x.exp_m1()`    | `expm1`     |
//! | `__torajs_math_log`      | `x.ln()`        | `log`       |
//! | `__torajs_math_log2`     | `x.log2()`      | `log2`      |
//! | `__torajs_math_log10`    | `x.log10()`     | `log10`     |
//! | `__torajs_math_log1p`    | `x.ln_1p()`     | `log1p`     |
//! | `__torajs_math_sin`      | `x.sin()`       | `sin`       |
//! | `__torajs_math_cos`      | `x.cos()`       | `cos`       |
//! | `__torajs_math_tan`      | `x.tan()`       | `tan`       |
//! | `__torajs_math_asin`     | `x.asin()`      | `asin`      |
//! | `__torajs_math_acos`     | `x.acos()`      | `acos`      |
//! | `__torajs_math_atan`     | `x.atan()`      | `atan`      |
//! | `__torajs_math_sinh`     | `x.sinh()`      | `sinh`      |
//! | `__torajs_math_cosh`     | `x.cosh()`      | `cosh`      |
//! | `__torajs_math_tanh`     | `x.tanh()`      | `tanh`      |
//! | `__torajs_math_asinh`    | `x.asinh()`     | `asinh`     |
//! | `__torajs_math_acosh`    | `x.acosh()`     | `acosh`     |
//! | `__torajs_math_atanh`    | `x.atanh()`     | `atanh`     |
//! | `__torajs_math_pow`      | `x.powf(y)`     | `pow`       |
//! | `__torajs_math_min`      | `x.min(y)`      | `fmin`      |
//! | `__torajs_math_max`      | `x.max(y)`      | `fmax`      |
//! | `__torajs_math_atan2`    | `y.atan2(x)`    | `atan2`     |
//!
//! `min` / `max` use C `fmin` / `fmax` semantics (NaN treated as
//! sentinel — non-NaN arg returned). JS spec actually says
//! `Math.min(NaN, 5) === NaN`; the pre-port IR-emitted version
//! used libm and conformance was green — we preserve the libm
//! semantics bit-for-bit. Spec-correctness wedge belongs in a
//! later task (after the rewrite stabilizes).

// ============================================================
// Unary — f64 → f64
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sqrt(x: f64) -> f64 {
    x.sqrt()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_abs(x: f64) -> f64 {
    x.abs()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_floor(x: f64) -> f64 {
    x.floor()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_ceil(x: f64) -> f64 {
    x.ceil()
}
/// `Math.round(x)` — **JS spec semantics** (round half toward +∞),
/// NOT libc `round` (round half away from zero).
///
/// `Math.round(2.5)` === `3` (both agree); `Math.round(-2.5)` === `-2`
/// (spec) vs `-3` (libc). The pre-port C impl in `runtime_str.c` was
/// specifically `floor(x + 0.5)` to preserve the spec behavior; we
/// replicate that bit-for-bit here so the port doesn't silently
/// change negative-half rounding.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_round(x: f64) -> f64 {
    (x + 0.5).floor()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_trunc(x: f64) -> f64 {
    x.trunc()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cbrt(x: f64) -> f64 {
    x.cbrt()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_exp(x: f64) -> f64 {
    x.exp()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_expm1(x: f64) -> f64 {
    x.exp_m1()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log(x: f64) -> f64 {
    x.ln()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log2(x: f64) -> f64 {
    x.log2()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log10(x: f64) -> f64 {
    x.log10()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_log1p(x: f64) -> f64 {
    x.ln_1p()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sin(x: f64) -> f64 {
    x.sin()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cos(x: f64) -> f64 {
    x.cos()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tan(x: f64) -> f64 {
    x.tan()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asin(x: f64) -> f64 {
    x.asin()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acos(x: f64) -> f64 {
    x.acos()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atan(x: f64) -> f64 {
    x.atan()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sinh(x: f64) -> f64 {
    x.sinh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_cosh(x: f64) -> f64 {
    x.cosh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_tanh(x: f64) -> f64 {
    x.tanh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_asinh(x: f64) -> f64 {
    x.asinh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_acosh(x: f64) -> f64 {
    x.acosh()
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atanh(x: f64) -> f64 {
    x.atanh()
}

// ============================================================
// Binary — f64 × f64 → f64
// ============================================================

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_pow(x: f64, y: f64) -> f64 {
    x.powf(y)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_min(x: f64, y: f64) -> f64 {
    x.min(y)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_max(x: f64, y: f64) -> f64 {
    x.max(y)
}

/// **Arg order**: ES `Math.atan2(y, x)` — i.e. `y` is first.
/// Matches `f64::atan2(self=y, other=x)` Rust convention.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_atan2(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/// `Math.sign(x)` — JS spec: `+1` / `-1` / preserve-zero. libc has
/// no `sign`; spec preserves `-0` / `+0` (not just returning 0) so
/// the C runtime had its own implementation. Rust `f64::signum`
/// returns `1.0` for `+0.0` and `-1.0` for `-0.0` (wrong); we
/// preserve the JS spec form: zero (any sign) returns itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        // x is +0 / -0 / NaN; preserve x. NaN flows through too.
        x
    }
}

// ============================================================
// PRNG + bit/precision intrinsics
// ============================================================

/// xorshift128+ thread-local state. Two u64 words, period 2^128 - 1.
/// Same algorithm V8 / SpiderMonkey use under the hood for
/// `Math.random()`. Replaces the pre-port libc `rand()` which has
/// poor distribution (low-bit periodicity) and pulls in a libc
/// rand symbol dep we don't want under the in-house pillar.
///
/// State is `(0, 0)` until first use, then lazily seeded from
/// `SystemTime::now()` via splitmix64. Per-thread Cell mirrors the
/// V8 design — single-threaded JS execution model, no contention.
use std::cell::Cell;

thread_local! {
    static RNG_STATE: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
}

#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

fn seed_rng_state() -> (u64, u64) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0xdead_beef_cafe_babe);
    let s0 = splitmix64(nanos.wrapping_add(0x9e3779b97f4a7c15));
    let s1 = splitmix64(s0.wrapping_add(0x9e3779b97f4a7c15));
    // xorshift requires non-zero state — splitmix64 of any non-zero
    // seed is non-zero, but be defensive.
    if s0 == 0 && s1 == 0 { (1, 0) } else { (s0, s1) }
}

/// `Math.random()` — uniform [0, 1) f64.
///
/// xorshift128+ → high 53 bits → multiply by 2^-53. Matches the
/// V8 / SpiderMonkey conversion path bit-for-bit, modulo the
/// per-process seed (we use system time + splitmix64; they use
/// /dev/urandom + crypto-quality seed).
///
/// JS spec wording is "implementation-defined" so the seed choice
/// is conformant; only the [0, 1) range is mandatory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_random() -> f64 {
    RNG_STATE.with(|cell| {
        let mut state = cell.get();
        if state == (0, 0) {
            state = seed_rng_state();
        }
        let (s0_old, s1_old) = state;
        let mut x = s0_old;
        let y = s1_old;
        x ^= x << 23;
        let new_s1 = x ^ y ^ (x >> 17) ^ (y >> 26);
        cell.set((y, new_s1));
        let raw = new_s1.wrapping_add(y);
        ((raw >> 11) as f64) * (1.0_f64 / ((1u64 << 53) as f64))
    })
}

/// `Math.imul(a, b)` — 32-bit signed integer multiplication, low 32
/// bits, sign-extended. ES spec form.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_imul(a: i64, b: i64) -> i64 {
    let r = (a as i32).wrapping_mul(b as i32);
    r as i64
}

/// `Math.clz32(x)` — count leading zeros of x's 32-bit unsigned
/// representation. Returns 32 if x is zero (Rust `leading_zeros`
/// already handles this case natively — no special branch needed
/// vs the pre-port C version that had to guard against
/// `__builtin_clz(0)` being UB).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_clz32(x: i64) -> i64 {
    let v = x as i32 as u32;
    v.leading_zeros() as i64
}

/// `Math.fround(x)` — round x to the nearest f32 then back to f64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_math_fround(x: f64) -> f64 {
    x as f32 as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unary_basic() {
        assert_eq!(unsafe { __torajs_math_sqrt(4.0) }, 2.0);
        assert_eq!(unsafe { __torajs_math_abs(-7.5) }, 7.5);
        assert_eq!(unsafe { __torajs_math_floor(3.7) }, 3.0);
        assert_eq!(unsafe { __torajs_math_ceil(3.2) }, 4.0);
        assert_eq!(unsafe { __torajs_math_round(2.5) }, 3.0);
        assert_eq!(unsafe { __torajs_math_trunc(3.7) }, 3.0);
        assert_eq!(unsafe { __torajs_math_cbrt(27.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_exp(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_log(1.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_log2(8.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_log10(1000.0) }, 3.0);
    }

    #[test]
    fn trig_basic() {
        assert_eq!(unsafe { __torajs_math_sin(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_cos(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_tan(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_asin(0.0) }, 0.0);
        assert!((unsafe { __torajs_math_acos(0.0) } - core::f64::consts::FRAC_PI_2).abs() < 1e-15);
        assert_eq!(unsafe { __torajs_math_atan(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_sinh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_cosh(0.0) }, 1.0);
        assert_eq!(unsafe { __torajs_math_tanh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_asinh(0.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_acosh(1.0) }, 0.0);
        assert_eq!(unsafe { __torajs_math_atanh(0.0) }, 0.0);
    }

    #[test]
    fn binary_basic() {
        assert_eq!(unsafe { __torajs_math_pow(2.0, 10.0) }, 1024.0);
        assert_eq!(unsafe { __torajs_math_min(3.0, 5.0) }, 3.0);
        assert_eq!(unsafe { __torajs_math_max(3.0, 5.0) }, 5.0);
        assert_eq!(unsafe { __torajs_math_atan2(0.0, 1.0) }, 0.0);
    }

    #[test]
    fn random_range_and_sequence() {
        // [0, 1) — never == 1, may == 0 (1-in-2^53 chance, ignore).
        for _ in 0..1000 {
            let r = unsafe { __torajs_math_random() };
            assert!((0.0..1.0).contains(&r), "{r}");
        }
        // Two successive calls in the same thread must differ
        // (collision odds 2^-53 — negligible).
        let a = unsafe { __torajs_math_random() };
        let b = unsafe { __torajs_math_random() };
        assert_ne!(a, b);
    }

    #[test]
    fn imul_low32_signed_mul() {
        assert_eq!(unsafe { __torajs_math_imul(2, 3) }, 6);
        assert_eq!(unsafe { __torajs_math_imul(-1, -1) }, 1);
        // 0xffff_ffff * 5 = 0x4_ffff_fffb → low32 = 0xffff_fffb → -5
        assert_eq!(unsafe { __torajs_math_imul(0xffff_ffff, 5) }, -5);
        // INT32_MIN * INT32_MIN — overflows; low 32 bits = 0
        assert_eq!(
            unsafe { __torajs_math_imul(i32::MIN as i64, i32::MIN as i64) },
            0
        );
    }

    #[test]
    fn clz32_zero_and_one() {
        assert_eq!(unsafe { __torajs_math_clz32(0) }, 32);
        assert_eq!(unsafe { __torajs_math_clz32(1) }, 31);
        assert_eq!(unsafe { __torajs_math_clz32(0x8000_0000) }, 0);
        // negative i64 trimmed to i32 must wrap into u32 with high
        // bit set → clz of any non-zero u32 with high bit set = 0
        assert_eq!(unsafe { __torajs_math_clz32(-1) }, 0);
    }

    #[test]
    fn fround_loses_precision() {
        assert_eq!(unsafe { __torajs_math_fround(1.0) }, 1.0);
        // 0.1 in f64 is closest to 0.1, but rounding to f32 then
        // back changes the lower mantissa bits.
        let a = unsafe { __torajs_math_fround(0.1) };
        assert_ne!(a, 0.1);
        assert!((a - 0.1).abs() < 1e-7);
    }

    #[test]
    fn nan_paths_via_libm() {
        // sqrt of negative → NaN
        assert!(unsafe { __torajs_math_sqrt(-1.0) }.is_nan());
        // acos of out-of-range → NaN
        assert!(unsafe { __torajs_math_acos(2.0) }.is_nan());
        // log of negative → NaN
        assert!(unsafe { __torajs_math_log(-1.0) }.is_nan());
        // expm1 of 0 → 0
        assert_eq!(unsafe { __torajs_math_expm1(0.0) }, 0.0);
    }
}
