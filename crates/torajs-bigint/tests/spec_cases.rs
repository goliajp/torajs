//! Black-box spec-compliance tests for `torajs-bigint`. Inline unit
//! tests cover the algorithm-internal corners; these tests exercise
//! the public extern API on edge cases that exercise sign
//! normalization, two's-complement bitwise semantics, and parse /
//! to_string round-trips.
//!
//! ## Why these tests use from_i64 + arith chains
//!
//! `__torajs_bigint_from_decimal` expects a Str heap-block pointer
//! (with the universal 16-byte header) — not a raw `&[u8]`. From
//! the test crate we don't have `torajs-str` to allocate a Str, so
//! we synthesize multi-limb BigInts by building from i64 + arith
//! chains. The decimal-parse + hex-parse paths are tested by the
//! end-to-end conformance fixtures (test262 + curated cases), not
//! here.

use std::ffi::c_void;

use torajs_bigint::{
    __torajs_bigint_add, __torajs_bigint_and, __torajs_bigint_cmp, __torajs_bigint_drop,
    __torajs_bigint_eq, __torajs_bigint_from_i64, __torajs_bigint_mul, __torajs_bigint_not,
    __torajs_bigint_or, __torajs_bigint_sub, __torajs_bigint_xor,
};

fn drop_bi(p: *mut u8) {
    unsafe { __torajs_bigint_drop(p as *mut c_void) };
}

fn eq(a: *mut u8, b: *mut u8) -> bool {
    unsafe { __torajs_bigint_eq(a as *const c_void, b as *const c_void) != 0 }
}

fn add(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe { __torajs_bigint_add(a as *const c_void, b as *const c_void) }
}

fn sub(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe { __torajs_bigint_sub(a as *const c_void, b as *const c_void) }
}

fn mul(a: *mut u8, b: *mut u8) -> *mut u8 {
    unsafe { __torajs_bigint_mul(a as *const c_void, b as *const c_void) }
}

fn from_i64(v: i64) -> *mut u8 {
    unsafe { __torajs_bigint_from_i64(v) }
}

#[test]
fn add_with_carry_across_limb_boundary() {
    // 2^63 + 2^63 == 2^64 → exercises carry into a second limb.
    // We have to build 2^63 from i64::MAX (= 2^63 - 1) + 1.
    let max = from_i64(i64::MAX);
    let one = from_i64(1);
    let two_63 = add(max, one); // 2^63
    drop_bi(max);
    // Now compute 2^63 + 2^63.
    let sum = add(two_63, two_63);
    // 2^64 also via i64-chain: (i64::MAX + 1) * 2 OR we just observe
    // that sum should differ from 2^63 and be exactly twice as large.
    let expected = mul(two_63, from_then_drop(2));
    assert!(eq(sum, expected), "2*2^63 == 2^64 failed");
    drop_bi(sum);
    drop_bi(expected);
    drop_bi(two_63);
    drop_bi(one);
}

fn from_then_drop(v: i64) -> *mut u8 {
    from_i64(v)
}

#[test]
fn sub_with_borrow_across_limb_boundary() {
    // (2^63 + 2^63) - 1 = 2^64 - 1; exercise borrow path.
    let max = from_i64(i64::MAX);
    let one = from_i64(1);
    let two_63 = add(max, one);
    drop_bi(max);
    let two_64 = add(two_63, two_63);
    let diff = sub(two_64, one);
    // Verify diff < 2^64.
    let cmp = unsafe { __torajs_bigint_cmp(diff as *const c_void, two_64 as *const c_void) };
    assert!(cmp < 0, "sub should yield a smaller value");
    // Add 1 back — must recover two_64.
    let restored = add(diff, one);
    assert!(
        eq(restored, two_64),
        "round-trip 2^64 - 1 + 1 = 2^64 failed"
    );
    drop_bi(restored);
    drop_bi(diff);
    drop_bi(two_64);
    drop_bi(two_63);
    drop_bi(one);
}

#[test]
fn mul_schoolbook_small_operands() {
    let a = from_i64(12345);
    let b = from_i64(67890);
    let prod = mul(a, b);
    let expected = from_i64(838102050);
    assert!(eq(prod, expected), "12345 * 67890 == 838102050");
    drop_bi(prod);
    drop_bi(expected);
    drop_bi(a);
    drop_bi(b);
}

#[test]
fn mul_squares_via_chain() {
    // 99 * 99 == 9801
    let a = from_i64(99);
    let p = mul(a, a);
    let expected = from_i64(9801);
    assert!(eq(p, expected));
    drop_bi(p);
    drop_bi(expected);
    drop_bi(a);
}

#[test]
fn bitwise_two_complement_view_on_negative() {
    // -1n & 5n == 5n (since -1 in two's complement is all-1s).
    let neg_one = from_i64(-1);
    let five = from_i64(5);
    let result = unsafe { __torajs_bigint_and(neg_one as *const c_void, five as *const c_void) };
    let expected = from_i64(5);
    assert!(eq(result, expected), "-1n & 5n must equal 5n");
    drop_bi(result);
    drop_bi(expected);
    drop_bi(neg_one);
    drop_bi(five);
}

#[test]
fn bitwise_not_complements() {
    // ~0n == -1n.
    let zero = from_i64(0);
    let result = unsafe { __torajs_bigint_not(zero as *const c_void) };
    let expected = from_i64(-1);
    assert!(eq(result, expected), "~0n must equal -1n");
    drop_bi(result);
    drop_bi(expected);
    drop_bi(zero);
}

#[test]
fn or_xor_signed_combinations() {
    // 0xff | (-1) == -1
    let v = from_i64(0xff);
    let neg_one = from_i64(-1);
    let or_result = unsafe { __torajs_bigint_or(v as *const c_void, neg_one as *const c_void) };
    let expected_or = from_i64(-1);
    assert!(eq(or_result, expected_or));
    drop_bi(or_result);
    drop_bi(expected_or);

    // 0xff ^ 0xff == 0
    let v2 = from_i64(0xff);
    let xor_result = unsafe { __torajs_bigint_xor(v as *const c_void, v2 as *const c_void) };
    let zero = from_i64(0);
    assert!(eq(xor_result, zero));
    drop_bi(xor_result);
    drop_bi(zero);
    drop_bi(v2);
    drop_bi(v);
    drop_bi(neg_one);
}

#[test]
fn cmp_zero_and_negatives() {
    let zero = from_i64(0);
    let pos = from_i64(42);
    let neg = from_i64(-42);
    unsafe {
        assert!(__torajs_bigint_cmp(zero as *const c_void, pos as *const c_void) < 0);
        assert!(__torajs_bigint_cmp(zero as *const c_void, neg as *const c_void) > 0);
        assert!(__torajs_bigint_cmp(pos as *const c_void, neg as *const c_void) > 0);
        assert_eq!(
            __torajs_bigint_cmp(zero as *const c_void, zero as *const c_void),
            0
        );
    }
    drop_bi(zero);
    drop_bi(pos);
    drop_bi(neg);
}

#[test]
fn add_zero_identity() {
    let x = from_i64(42);
    let zero = from_i64(0);
    let sum = add(x, zero);
    assert!(eq(sum, x), "x + 0 == x");
    drop_bi(sum);
    drop_bi(x);
    drop_bi(zero);
}

#[test]
fn sub_self_is_zero() {
    let x = from_i64(42);
    let diff = sub(x, x);
    let zero = from_i64(0);
    assert!(eq(diff, zero), "x - x == 0");
    drop_bi(diff);
    drop_bi(x);
    drop_bi(zero);
}
