//! Black-box tests for `torajs-str` covering the spec-edge corners
//! of the public ABI. Uses `StrBlock::alloc` to build Str heap blocks
//! directly (cheaper than going through the IR-emit path).

use torajs_str::{
    __torajs_str_concat, __torajs_str_eq, __torajs_str_free, __torajs_str_slice, StrBlock,
};

fn make_str(payload: &[u8]) -> *mut u8 {
    let mut b = StrBlock::alloc(payload.len() as u64);
    let dst = unsafe { b.as_bytes_mut(payload.len() as u64) };
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
