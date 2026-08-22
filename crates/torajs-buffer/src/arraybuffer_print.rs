//! `console.log` wire form for an ArrayBuffer
//! (RFC 20260823-typedarray-substrate 刀 1).
//!
//! `ArrayBuffer(N) [ b, b, … ]`, and `ArrayBuffer(0) []` when there
//! is nothing to list — bun's shape, which shows the bytes and not
//! the maximum, so a resizable buffer prints exactly like a
//! fixed-length one of the same current length. A detached buffer
//! has no bytes to show and prints as length zero.

use core::ffi::c_void;

use crate::arraybuffer::{byte_len, data_ptr};

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
}

#[inline]
unsafe fn put(bytes: &[u8]) {
    for &b in bytes {
        unsafe { __torajs_io_putc_out(b as i32) };
    }
}

/// Decimal, no allocation — the values are single bytes, so three
/// digits is the whole range.
unsafe fn put_u8(n: u8) {
    let mut buf = [0u8; 3];
    let mut i = 3;
    let mut v = n;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10);
        v /= 10;
        if v == 0 {
            break;
        }
    }
    unsafe { put(&buf[i..]) };
}

unsafe fn put_i64(mut n: i64) {
    if n < 0 {
        unsafe { put(b"-") };
        n = -n;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    unsafe { put(&buf[i..]) };
}

/// # Safety
/// `cell` is a live ArrayBuffer cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arraybuffer_print(cell: *mut c_void) {
    unsafe {
        let data = data_ptr(cell);
        let len = if data.is_null() { 0 } else { byte_len(cell) };
        put(b"ArrayBuffer(");
        put_i64(len);
        if len == 0 {
            put(b") []");
            return;
        }
        put(b") [ ");
        for i in 0..len {
            if i != 0 {
                put(b", ");
            }
            put_u8(data.add(i as usize).read());
        }
        put(b" ]");
    }
}
