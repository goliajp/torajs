//! IsLooselyEqual (`==` / `!=`) per ES §7.2.14 over [`AnyValue`]
//! operands. RFC 20260713-loose-eq-substrate blade 1.
//!
//! Same-bucket pairs delegate to strict equality (§7.2.14 step 1);
//! cross-bucket pairs walk the coercion ladder:
//!
//! - nullish × non-nullish → `false` (steps 2-4; nullish × nullish
//!   is `true` via the same-bucket arm)
//! - Boolean side → ToNumber, recurse (steps 9-10)
//! - Number × String → ToNumber(string), numeric compare (steps 5-6)
//! - BigInt × String → StringToBigInt, exact compare (steps 7-8;
//!   invalid grammar → `false`)
//! - BigInt × Number → exact mathematical-value compare (step 13 —
//!   the f64 side must be a finite integer; the BigInt is NOT
//!   rounded through f64, so 2^53+ magnitudes compare exactly)
//! - primitive × Object → ToPrimitive(object) with default hint,
//!   recurse (steps 11-12; a pending TypeError from a
//!   both-methods-answered-objects coercion surfaces through the
//!   caller's throw check)
//! - Symbol × other-primitive → `false` (step 14)

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_pointer, as_void_ptr, box_double, is_bool, is_cell,
    is_double, is_int32, is_null, is_short_str, is_undefined,
};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_strict_eq};
use crate::nanbox_ffi_materialize::{drop_materialized_str, materialize_short_str};

/// BigInt kernel FFI — resolved from `libtorajs_bigint.a` in the
/// shipped link. The unit-test binary must not pull these past
/// `-dead_strip` (mirrors `to_primitive::dispatch_method`), so
/// tests get never-equal stubs; the BigInt arms are exercised by
/// conformance, not unit tests.
#[cfg(not(test))]
pub(crate) mod bigint_ffi {
    use core::ffi::c_void;
    unsafe extern "C" {
        pub fn __torajs_bigint_eq(a: *const c_void, b: *const c_void) -> i64;
        pub fn __torajs_bigint_cmp(a: *const c_void, b: *const c_void) -> i64;
        pub fn __torajs_bigint_from_number(v: f64) -> *mut u8;
        pub fn __torajs_bigint_from_str_strict(s: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_drop(p: *mut c_void);
        pub fn __torajs_bigint_is_nonzero(p: *const c_void) -> i64;
        pub fn __torajs_bigint_add(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_sub(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_mul(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_div(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_mod(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_pow(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_and(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_or(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_xor(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_shl(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_shr(a: *const c_void, b: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_not(a: *const c_void) -> *mut u8;
        pub fn __torajs_bigint_neg(a: *const c_void) -> *mut u8;
    }
}

#[cfg(test)]
pub(crate) mod bigint_ffi {
    use core::ffi::c_void;
    pub unsafe extern "C" fn __torajs_bigint_eq(_a: *const c_void, _b: *const c_void) -> i64 {
        0
    }
    pub unsafe extern "C" fn __torajs_bigint_cmp(_a: *const c_void, _b: *const c_void) -> i64 {
        0
    }
    pub unsafe extern "C" fn __torajs_bigint_from_number(_v: f64) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_from_str_strict(_s: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_drop(_p: *mut c_void) {}
    pub unsafe extern "C" fn __torajs_bigint_is_nonzero(_p: *const c_void) -> i64 {
        1
    }
    pub unsafe extern "C" fn __torajs_bigint_add(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_sub(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_mul(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_div(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_mod(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_pow(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_and(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_or(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_xor(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_shl(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_shr(_a: *const c_void, _b: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_not(_a: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
    pub unsafe extern "C" fn __torajs_bigint_neg(_a: *const c_void) -> *mut u8 {
        core::ptr::null_mut()
    }
}

/// §7.2.14 type buckets. `Num` fuses the i32/f64 encodings (one
/// mathematical Number type); `Str` fuses ShortStr and heap Str.
#[derive(Clone, Copy, PartialEq)]
enum Bucket {
    Num,
    Str,
    Bool,
    Nullish,
    BigInt,
    Sym,
    Obj,
}

unsafe fn bucket(v: AnyValue) -> Bucket {
    if is_int32(v) || is_double(v) {
        return Bucket::Num;
    }
    if is_short_str(v) {
        return Bucket::Str;
    }
    if is_bool(v) {
        return Bucket::Bool;
    }
    if is_null(v) || is_undefined(v) {
        return Bucket::Nullish;
    }
    if is_cell(v) {
        // SAFETY: cell pointer non-null per is_cell.
        let h = unsafe { &*as_pointer(v) };
        return match h.tag() {
            Tag::Str => Bucket::Str,
            Tag::BigInt => Bucket::BigInt,
            Tag::Symbol => Bucket::Sym,
            _ => Bucket::Obj,
        };
    }
    // Non-cell, non-immediate encodings don't exist in a
    // well-formed runtime; identity semantics is the harmless
    // answer (mirrors anyv_strict_eq's trailing `false`).
    debug_assert!(false, "unknown AnyValue encoding");
    Bucket::Obj
}

fn num_val(v: AnyValue) -> f64 {
    if is_int32(v) {
        as_int32(v) as f64
    } else {
        as_double(v)
    }
}

/// `IsLooselyEqual(l, r)` per ES §7.2.14.
///
/// # Safety
///
/// Cell-case operands must point to valid heap objects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_loose_eq(l: AnyValue, r: AnyValue) -> bool {
    unsafe { loose_eq(l, r) }
}

unsafe fn loose_eq(l: AnyValue, r: AnyValue) -> bool {
    let bl = unsafe { bucket(l) };
    let br = unsafe { bucket(r) };
    if bl == br {
        return match bl {
            Bucket::Nullish => true,
            // SAFETY: both cells are Tag::BigInt per bucket.
            Bucket::BigInt => unsafe {
                bigint_ffi::__torajs_bigint_eq(
                    as_pointer(l) as *const c_void,
                    as_pointer(r) as *const c_void,
                ) != 0
            },
            // §7.2.14 step 1 — same type delegates to
            // IsStrictlyEqual; anyv_strict_eq covers every
            // same-bucket rule (NaN, ±0, str byte-eq, identity).
            _ => unsafe { __torajs_anyv_strict_eq(l, r) },
        };
    }
    match (bl, br) {
        // Steps 2-4 — nullish only equals nullish (same-bucket arm).
        (Bucket::Nullish, _) | (_, Bucket::Nullish) => false,
        // Steps 9-10 — Boolean side coerces to Number, recurse.
        (Bucket::Bool, _) => unsafe { loose_eq(box_double(if as_bool(l) { 1.0 } else { 0.0 }), r) },
        (_, Bucket::Bool) => unsafe { loose_eq(l, box_double(if as_bool(r) { 1.0 } else { 0.0 })) },
        // Steps 5-6 — Number × String coerces the string side.
        (Bucket::Num, Bucket::Str) => num_val(l) == unsafe { str_num(r) },
        (Bucket::Str, Bucket::Num) => (unsafe { str_num(l) }) == num_val(r),
        // Steps 7-8 — BigInt × String via StringToBigInt.
        (Bucket::BigInt, Bucket::Str) => unsafe { bigint_str_eq(l, r) },
        (Bucket::Str, Bucket::BigInt) => unsafe { bigint_str_eq(r, l) },
        // Step 13 — BigInt × Number, exact mathematical value.
        (Bucket::BigInt, Bucket::Num) => unsafe { bigint_num_eq(l, num_val(r)) },
        (Bucket::Num, Bucket::BigInt) => unsafe { bigint_num_eq(r, num_val(l)) },
        // Steps 11-12 — Object × primitive: ToPrimitive, recurse.
        (Bucket::Obj, _) => unsafe { obj_prim_eq(l, r) },
        (_, Bucket::Obj) => unsafe { obj_prim_eq(r, l) },
        // Step 14 — Symbol × {Num, Str, BigInt} never equal.
        _ => false,
    }
}

/// `ToNumber(string)` for either Str encoding.
unsafe fn str_num(v: AnyValue) -> f64 {
    if is_short_str(v) {
        // SAFETY: ShortStr materializes to a fresh rc=1 Str.
        let s = unsafe { materialize_short_str(v) };
        let n = unsafe { crate::__torajs_str_to_number(s as *const c_void) };
        unsafe { drop_materialized_str(s) };
        return n;
    }
    // SAFETY: caller guarantees a Tag::Str cell.
    unsafe { crate::__torajs_str_to_number(as_pointer(v) as *const c_void) }
}

/// §7.2.14 steps 7-8 — `bigint == string`: StringToBigInt (strict
/// grammar; NULL = spec "undefined") then exact compare.
unsafe fn bigint_str_eq(b: AnyValue, s: AnyValue) -> bool {
    let (sp, materialized) = if is_short_str(s) {
        // SAFETY: ShortStr materializes to a fresh rc=1 Str.
        let m = unsafe { materialize_short_str(s) };
        (m as *const c_void, Some(m))
    } else {
        (as_pointer(s) as *const c_void, None)
    };
    // SAFETY: sp is a valid Str pointer either way.
    let n = unsafe { bigint_ffi::__torajs_bigint_from_str_strict(sp) };
    if let Some(m) = materialized {
        // SAFETY: m is the freshly-materialized Str we own.
        unsafe { drop_materialized_str(m) };
    }
    if n.is_null() {
        return false;
    }
    // SAFETY: b is a Tag::BigInt cell; n is a fresh BigInt block.
    let eq = unsafe {
        bigint_ffi::__torajs_bigint_eq(as_pointer(b) as *const c_void, n as *const c_void) != 0
    };
    unsafe { bigint_ffi::__torajs_bigint_drop(n as *mut c_void) };
    eq
}

/// §7.2.14 step 13 — `bigint == number`: a non-finite or
/// non-integral Number never equals a BigInt; an integral f64
/// converts exactly (mantissa/exponent decomposition — no
/// precision loss at 2^53+).
unsafe fn bigint_num_eq(b: AnyValue, n: f64) -> bool {
    if !n.is_finite() || n.floor() != n {
        return false;
    }
    // SAFETY: the guard above keeps from_number off its
    // pending-RangeError path (finite integral input only).
    let tmp = unsafe { bigint_ffi::__torajs_bigint_from_number(n) };
    if tmp.is_null() {
        return false;
    }
    // SAFETY: b is a Tag::BigInt cell; tmp is a fresh BigInt block.
    let eq = unsafe {
        bigint_ffi::__torajs_bigint_eq(as_pointer(b) as *const c_void, tmp as *const c_void) != 0
    };
    unsafe { bigint_ffi::__torajs_bigint_drop(tmp as *mut c_void) };
    eq
}

/// §7.2.14 steps 11-12 — object × primitive: `ToPrimitive(obj)`
/// with default hint (number order for ordinary objects, string
/// order for Date per §21.4.4.45), recurse on the primitive
/// result. A pending TypeError (both coercion methods answered
/// objects) surfaces through the caller's throw check; `false` is
/// the placeholder.
unsafe fn obj_prim_eq(obj: AnyValue, other: AnyValue) -> bool {
    // SAFETY: obj is a non-Str/BigInt/Symbol cell per bucket.
    match unsafe { crate::to_primitive::heap_to_primitive_default(as_void_ptr(obj)) } {
        Some(prim) => {
            let eq = unsafe { loose_eq(prim, other) };
            unsafe { __torajs_anyv_rc_dec(prim) };
            eq
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{VALUE_FALSE, VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED, box_int32};

    unsafe fn eq(l: AnyValue, r: AnyValue) -> bool {
        unsafe { __torajs_anyv_loose_eq(l, r) }
    }

    #[test]
    fn nullish_pairs() {
        unsafe {
            assert!(eq(VALUE_NULL, VALUE_UNDEFINED));
            assert!(eq(VALUE_UNDEFINED, VALUE_NULL));
            assert!(eq(VALUE_NULL, VALUE_NULL));
            assert!(!eq(VALUE_NULL, box_int32(0)));
            assert!(!eq(VALUE_UNDEFINED, VALUE_FALSE));
        }
    }

    #[test]
    fn bool_number_coercion() {
        unsafe {
            assert!(eq(VALUE_TRUE, box_int32(1)));
            assert!(eq(box_int32(0), VALUE_FALSE));
            assert!(!eq(VALUE_TRUE, box_int32(2)));
            assert!(eq(VALUE_TRUE, box_double(1.0)));
        }
    }

    #[test]
    fn number_pairs() {
        unsafe {
            assert!(eq(box_int32(1), box_double(1.0)));
            assert!(!eq(box_double(f64::NAN), box_double(f64::NAN)));
            assert!(eq(box_double(0.0), box_double(-0.0)));
        }
    }

    #[test]
    fn string_number_coercion() {
        // ShortStr "1" → materialize → str_to_number test stub
        // answers 42.0 (lib.rs sentinel), so the observable claim
        // here is only "the Str×Num arm routes through ToNumber":
        // 42 == 42 must hold, 1 == "1" can't be asserted in the
        // unit domain (conformance covers it).
        unsafe {
            let s = crate::nanbox::try_box_short_str(b"42").unwrap();
            assert!(eq(box_int32(42), s));
            assert!(eq(s, box_double(42.0)));
            assert!(!eq(box_int32(7), s));
        }
    }
}
