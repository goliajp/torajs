//! Black-box tests for `torajs-str` covering the spec-edge corners
//! of the public ABI. Uses `StrBlock::alloc` to build Str heap blocks
//! directly (cheaper than going through the IR-emit path).

use torajs_str::{
    __torajs_str_concat, __torajs_str_eq, __torajs_str_free, __torajs_str_slice, StrBlock,
};

// Transitive extern: torajs-rc's `__torajs_rc_dec` notifies the weak
// registry on death, and the RC-4 F1b-2 NULL-operand path in
// `__torajs_str_concat` pulls `str_drop` → `rc_dec` into this test
// binary's live set. No-op stub (not panicking) — same convention as
// torajs-value-drop's integration-test stub; the crate's own
// `#[cfg(test)]` stub in lib.rs doesn't reach separate test binaries.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}

fn make_str(payload: &[u8]) -> *mut u8 {
    let mut b = StrBlock::alloc(payload.len() as u32);
    let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
    dst.copy_from_slice(payload);
    b.into_raw()
}

fn read_str(p: *const u8) -> Vec<u8> {
    let len = unsafe { *(p.add(8) as *const u64) };
    let bytes = unsafe { std::slice::from_raw_parts(p.add(16), len as usize) };
    bytes.to_vec()
}

#[test]
fn concat_empty_left_returns_right() {
    let a = make_str(b"");
    let b = make_str(b"hello");
    let r = unsafe { __torajs_str_concat(a, b) };
    assert_eq!(read_str(r), b"hello");
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
        __torajs_str_free(r);
    }
}

#[test]
fn concat_empty_right_returns_left() {
    let a = make_str(b"hello");
    let b = make_str(b"");
    let r = unsafe { __torajs_str_concat(a, b) };
    assert_eq!(read_str(r), b"hello");
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
        __torajs_str_free(r);
    }
}

#[test]
fn concat_both_empty_yields_empty() {
    let a = make_str(b"");
    let b = make_str(b"");
    let r = unsafe { __torajs_str_concat(a, b) };
    assert_eq!(read_str(r), b"");
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
        __torajs_str_free(r);
    }
}

#[test]
fn slice_negative_start_normalizes_to_offset_from_end() {
    // String("hello").slice(-3) == "llo"
    let s = make_str(b"hello");
    let r = unsafe { __torajs_str_slice(s, -3, 5) };
    assert_eq!(read_str(r), b"llo");
    unsafe {
        __torajs_str_free(s);
        __torajs_str_free(r);
    }
}

#[test]
fn slice_oob_end_clamps_to_length() {
    // String("hello").slice(0, 100) == "hello"
    let s = make_str(b"hello");
    let r = unsafe { __torajs_str_slice(s, 0, 100) };
    assert_eq!(read_str(r), b"hello");
    unsafe {
        __torajs_str_free(s);
        __torajs_str_free(r);
    }
}

#[test]
fn slice_start_after_end_yields_empty() {
    // String("hello").slice(3, 1) == "" (per ES spec — empty, no swap)
    let s = make_str(b"hello");
    let r = unsafe { __torajs_str_slice(s, 3, 1) };
    assert_eq!(read_str(r), b"");
    unsafe {
        __torajs_str_free(s);
        __torajs_str_free(r);
    }
}

#[test]
fn eq_byte_equal_strings() {
    let a = make_str(b"hello");
    let b = make_str(b"hello");
    assert_ne!(a, b, "fresh allocs should produce distinct pointers");
    assert_eq!(
        unsafe { __torajs_str_eq(a, b) },
        1,
        "byte-equal strings ==="
    );
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
    }
}

#[test]
fn eq_different_strings() {
    let a = make_str(b"hello");
    let b = make_str(b"world");
    assert_eq!(unsafe { __torajs_str_eq(a, b) }, 0, "different bytes !==");
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
    }
}

#[test]
fn eq_different_lengths_short_circuit() {
    let a = make_str(b"hi");
    let b = make_str(b"hello");
    assert_eq!(
        unsafe { __torajs_str_eq(a, b) },
        0,
        "len-mismatched strings short-circuit !=="
    );
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(b);
    }
}

// RC-4 F1b-2 — a NULL ptr in a Str-typed slot denotes the JS
// `undefined` value (uncaptured regex groups, `[.., undefined]`
// array-literal slots). eq is identity, concat is the text
// "undefined"; neither derefs the NULL.

#[test]
fn eq_null_operands_identity() {
    let s = make_str(b"undefined");
    assert_eq!(
        unsafe { __torajs_str_eq(std::ptr::null(), std::ptr::null()) },
        1,
        "undefined === undefined"
    );
    assert_eq!(
        unsafe { __torajs_str_eq(std::ptr::null(), s) },
        0,
        "undefined === \"undefined\" is a type mismatch, never content"
    );
    assert_eq!(unsafe { __torajs_str_eq(s, std::ptr::null()) }, 0);
    unsafe { __torajs_str_free(s) };
}

#[test]
fn concat_null_operand_is_undefined_text() {
    let a = make_str(b"x: ");
    let r = unsafe { __torajs_str_concat(a, std::ptr::null()) };
    assert_eq!(read_str(r), b"x: undefined");
    let l = unsafe { __torajs_str_concat(std::ptr::null(), a) };
    assert_eq!(read_str(l), b"undefinedx: ");
    let both = unsafe { __torajs_str_concat(std::ptr::null(), std::ptr::null()) };
    assert_eq!(read_str(both), b"undefinedundefined");
    unsafe {
        __torajs_str_free(a);
        __torajs_str_free(r);
        __torajs_str_free(l);
        __torajs_str_free(both);
    }
}
