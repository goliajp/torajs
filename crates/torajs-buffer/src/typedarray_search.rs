//! `indexOf` / `lastIndexOf` / `includes`
//! (RFC 20260823-typedarray-substrate 刀 5, slab A search half).
//!
//! Three methods, two equalities, and one place where the difference
//! between them is visible:
//!
//! - `indexOf` and `lastIndexOf` use IsStrictlyEqual, under which
//!   NaN matches nothing — so a `Float64Array` full of NaN answers
//!   -1 for `indexOf(NaN)`.
//! - `includes` uses SameValueZero, under which NaN matches NaN.
//!
//! Both treat `+0` and `-0` as the same value, so the two differ on
//! exactly one input.
//!
//! The other difference is what they do to a view whose buffer
//! shrank while an argument's `valueOf` was running. `indexOf` asks
//! HasProperty first (§23.2.3.14 step 8.a), and an index past the
//! current extent is simply absent — skipped. `includes` asks Get
//! (§23.2.3.13 step 7.a), and an index past the extent reads
//! `undefined` — which MATCHES when the search value is `undefined`.
//! So `ta.includes(undefined)` can answer true for a typed array,
//! whose elements are never undefined. That is not a quirk of this
//! implementation; it is what the two step lists say.
//!
//! Nothing here mints a value to compare against. Every non-BigInt
//! element fits an `f64` exactly, and the BigInt needle is
//! range-checked once, before the scan, rather than per element.

use torajs_anyvalue::nanbox::{
    AnyValue, VALUE_FALSE, VALUE_TRUE, VALUE_UNDEFINED, as_int32, as_void_ptr, is_cell, is_double,
    is_int32,
};
use torajs_rc::Tag;

use crate::typedarray::Kind;
use crate::typedarray_elem;
use crate::typedarray_span::{revalidate, to_integer_or_infinity, validate};

unsafe extern "C" {
    fn __torajs_bigint_from_i64(v: i64) -> *mut u8;
    fn __torajs_bigint_from_u64(v: u64) -> *mut u8;
    fn __torajs_bigint_to_u64_wrapping(p: *const core::ffi::c_void) -> u64;
    fn __torajs_bigint_eq(a: *const core::ffi::c_void, b: *const core::ffi::c_void) -> i64;
    fn __torajs_bigint_drop(p: *mut core::ffi::c_void);
}

/// What the search value reduces to, against a particular element
/// type. `Never` is not "not found yet" — it is "no element of this
/// type can be this value", which is the answer for a string needle
/// in a `Uint8Array` and for a Number needle in a `BigInt64Array`.
#[derive(Clone, Copy)]
enum Needle {
    Never,
    Num(f64),
    Bits(u64),
}

/// Reduce the search value once, before the scan.
///
/// For the BigInt kinds this is where the range check happens: the
/// wrapping low 64 bits of `2n ** 64n` are zero, so comparing raw
/// bits alone would report a match against an element holding 0.
/// Minting the round-trip once and asking BigInt equality settles it
/// for the whole scan rather than per element.
///
/// # Safety
/// `search` is a live AnyValue.
unsafe fn needle(kind: Kind, search: AnyValue) -> Needle {
    if kind.is_bigint() {
        if !is_cell(search) {
            return Needle::Never;
        }
        let ptr = as_void_ptr(search);
        let tag = unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() };
        if tag != Tag::BigInt as u16 {
            return Needle::Never;
        }
        unsafe {
            let raw = __torajs_bigint_to_u64_wrapping(ptr);
            let round = if kind == Kind::BigInt64 {
                __torajs_bigint_from_i64(raw as i64)
            } else {
                __torajs_bigint_from_u64(raw)
            };
            let same = __torajs_bigint_eq(ptr, round as *const core::ffi::c_void) != 0;
            __torajs_bigint_drop(round as *mut core::ffi::c_void);
            if same {
                Needle::Bits(raw)
            } else {
                Needle::Never
            }
        }
    } else if is_int32(search) {
        Needle::Num(f64::from(as_int32(search)))
    } else if is_double(search) {
        Needle::Num(torajs_anyvalue::nanbox::as_double(search))
    } else {
        Needle::Never
    }
}

/// Does element `i` equal the needle? `same_zero` picks
/// SameValueZero over IsStrictlyEqual, and the two differ on NaN
/// alone.
///
/// # Safety
/// `base` addresses at least `(i + 1)` elements of `kind`.
unsafe fn matches(base: *const u8, kind: Kind, i: i64, n: Needle, same_zero: bool) -> bool {
    match n {
        Needle::Never => false,
        Needle::Bits(raw) => unsafe { typedarray_elem::read_u64(base, i) == raw },
        Needle::Num(x) => {
            let e = unsafe { typedarray_elem::read_f64(base, kind, i) };
            e == x || (same_zero && e.is_nan() && x.is_nan())
        }
    }
}

/// The two lengths a search sees: the one it took before coercing
/// `fromIndex` (which bounds the loop) and the one that is true
/// afterwards (which decides whether each index is still there).
/// They differ only when a `valueOf` resized the buffer.
struct Scan {
    base: *mut u8,
    kind: Kind,
    /// Loop bound — §23.2.3.14 step 3's length.
    len: i64,
    /// Live extent — what HasProperty answers against.
    live: i64,
}

/// # Safety
/// `recv` is a live TypedArray AnyValue.
unsafe fn rescan(recv: AnyValue, len: i64) -> Option<Scan> {
    let span = unsafe { revalidate(recv) }?;
    Some(Scan {
        base: span.base,
        kind: span.kind,
        len,
        live: span.len,
    })
}

/// §23.2.3.14 `%TypedArray%.prototype.indexOf`.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_index_of(
    recv: AnyValue,
    search: AnyValue,
    from: AnyValue,
) -> f64 {
    unsafe {
        let Some(span) = validate(recv) else {
            return -1.0;
        };
        let len = span.len;
        if len == 0 {
            return -1.0;
        }
        let Some(n) = to_integer_or_infinity(from) else {
            return -1.0;
        };
        if n == f64::INFINITY {
            return -1.0;
        }
        let mut k = start_index(n, len);
        let Some(scan) = rescan(recv, len) else {
            return -1.0;
        };
        let needle = needle(scan.kind, search);
        while k < scan.len {
            if k < scan.live && matches(scan.base, scan.kind, k, needle, false) {
                return k as f64;
            }
            k += 1;
        }
        -1.0
    }
}

/// §23.2.3.19 `%TypedArray%.prototype.lastIndexOf`.
///
/// `has_from` is not a convenience: an ABSENT `fromIndex` starts at
/// `len - 1`, while an explicit `undefined` coerces to 0 and looks
/// at index 0 alone. Collapsing the two would make
/// `lastIndexOf(x, undefined)` answer what `lastIndexOf(x)` does.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_last_index_of(
    recv: AnyValue,
    search: AnyValue,
    from: AnyValue,
    has_from: i64,
) -> f64 {
    unsafe {
        let Some(span) = validate(recv) else {
            return -1.0;
        };
        let len = span.len;
        if len == 0 {
            return -1.0;
        }
        let n = if has_from != 0 {
            let Some(v) = to_integer_or_infinity(from) else {
                return -1.0;
            };
            v
        } else {
            (len - 1) as f64
        };
        if n == f64::NEG_INFINITY {
            return -1.0;
        }
        let mut k = if n >= 0.0 {
            n.min((len - 1) as f64) as i64
        } else {
            let v = len as f64 + n;
            if v < 0.0 {
                return -1.0;
            }
            v as i64
        };
        let Some(scan) = rescan(recv, len) else {
            return -1.0;
        };
        let needle = needle(scan.kind, search);
        while k >= 0 {
            if k < scan.live && matches(scan.base, scan.kind, k, needle, false) {
                return k as f64;
            }
            k -= 1;
        }
        -1.0
    }
}

/// §23.2.3.13 `%TypedArray%.prototype.includes`.
///
/// # Safety
/// The slots are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_includes(
    recv: AnyValue,
    search: AnyValue,
    from: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = validate(recv) else {
            return VALUE_FALSE;
        };
        let len = span.len;
        if len == 0 {
            return VALUE_FALSE;
        }
        let Some(n) = to_integer_or_infinity(from) else {
            return VALUE_FALSE;
        };
        if n == f64::INFINITY {
            return VALUE_FALSE;
        }
        let mut k = start_index(n, len);
        let Some(scan) = rescan(recv, len) else {
            return VALUE_FALSE;
        };
        let needle = needle(scan.kind, search);
        let undefined_needle = search == VALUE_UNDEFINED;
        while k < scan.len {
            if k >= scan.live {
                // Step 7.a is a Get, and past the live extent it
                // answers `undefined` rather than being absent.
                if undefined_needle {
                    return VALUE_TRUE;
                }
            } else if matches(scan.base, scan.kind, k, needle, true) {
                return VALUE_TRUE;
            }
            k += 1;
        }
        VALUE_FALSE
    }
}

/// The `fromIndex` clamp shared by `indexOf` and `includes`:
/// `-Infinity` is 0, a negative counts back from the end and floors
/// at 0, and a positive is used as is (the loop bound stops it).
fn start_index(n: f64, len: i64) -> i64 {
    if n == f64::NEG_INFINITY {
        return 0;
    }
    if n >= 0.0 {
        return if n > len as f64 { len } else { n as i64 };
    }
    let k = len as f64 + n;
    if k < 0.0 { 0 } else { k as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_index_clamp_matches_the_spec_steps() {
        assert_eq!(start_index(0.0, 5), 0);
        assert_eq!(start_index(3.0, 5), 3);
        assert_eq!(start_index(9.0, 5), 5);
        assert_eq!(start_index(-1.0, 5), 4);
        assert_eq!(start_index(-9.0, 5), 0);
        assert_eq!(start_index(f64::NEG_INFINITY, 5), 0);
    }
}
