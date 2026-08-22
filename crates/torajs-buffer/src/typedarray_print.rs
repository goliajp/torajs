//! `console.log` wire form for a typed array
//! (RFC 20260823-typedarray-substrate 刀 2).
//!
//! `Uint8Array(3) [ 1, 2, 3 ]`, and `Uint8Array(0) []` when empty —
//! bun's shape, the same as the ArrayBuffer printer's except that
//! the elements go through the element type rather than being raw
//! bytes, and the BigInt kinds carry the `n` suffix a BigInt literal
//! has.

use core::ffi::c_void;

use crate::typedarray::{Kind, kind_of, resolve};

unsafe extern "C" {
    fn __torajs_io_putc_out(c: i32) -> i32;
    /// The shortest-roundtrip formatter every other float printer in
    /// the runtime goes through, so a `Float64Array` element prints
    /// exactly like the same value would anywhere else.
    fn __torajs_fmt_dtoa(v: f64, buf: *mut u8, cap: i32) -> i32;
}

#[inline]
unsafe fn put(bytes: &[u8]) {
    for &b in bytes {
        unsafe { __torajs_io_putc_out(b as i32) };
    }
}

unsafe fn put_u64(mut n: u64) {
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

unsafe fn put_f64(v: f64) {
    // Negative zero renders as `0` HERE and only here. bun (and node,
    // and every V8/JSC console) prints `-0` for a bare value and
    // inside a plain array, but a typed array's formatter drops the
    // sign — the value itself is untouched, as `1 / ta[0]` being
    // -Infinity shows. This is a rendering convention, not a
    // coercion, so it lives in the printer and nowhere else.
    let v = if v == 0.0 { 0.0 } else { v };
    let mut buf = [0u8; 64];
    let n = unsafe { __torajs_fmt_dtoa(v, buf.as_mut_ptr(), 64) };
    if n > 0 {
        unsafe { put(&buf[..(n as usize).min(63)]) };
    }
}

unsafe fn put_i64(n: i64) {
    unsafe {
        if n < 0 {
            put(b"-");
            put_u64((n as i128).unsigned_abs() as u64);
        } else {
            put_u64(n as u64);
        }
    }
}

/// # Safety
/// `cell` is a live TypedArray cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_print(cell: *mut c_void) {
    unsafe {
        let kind = kind_of(cell);
        let resolved = resolve(cell);
        let len = resolved.map_or(0, |(_, l)| l);
        put(kind.name().as_bytes());
        put(b"(");
        put_i64(len);
        if len == 0 {
            put(b") []");
            return;
        }
        let (base, _) = resolved.unwrap();
        put(b") [ ");
        let esize = kind.element_size();
        for i in 0..len {
            if i != 0 {
                put(b", ");
            }
            let p = base.add((i * esize) as usize);
            match kind {
                Kind::Int8 => put_i64(i64::from(p.cast::<i8>().read_unaligned())),
                Kind::Uint8 | Kind::Uint8Clamped => put_u64(u64::from(p.read_unaligned())),
                Kind::Int16 => put_i64(i64::from(p.cast::<i16>().read_unaligned())),
                Kind::Uint16 => put_u64(u64::from(p.cast::<u16>().read_unaligned())),
                Kind::Int32 => put_i64(i64::from(p.cast::<i32>().read_unaligned())),
                Kind::Uint32 => put_u64(u64::from(p.cast::<u32>().read_unaligned())),
                Kind::Float16 => put_f64(crate::binary16::f16_bits_to_f64(
                    p.cast::<u16>().read_unaligned(),
                )),
                Kind::Float32 => put_f64(f64::from(p.cast::<f32>().read_unaligned())),
                Kind::Float64 => put_f64(p.cast::<f64>().read_unaligned()),
                Kind::BigInt64 => {
                    put_i64(p.cast::<i64>().read_unaligned());
                    put(b"n");
                }
                Kind::BigUint64 => {
                    put_u64(p.cast::<u64>().read_unaligned());
                    put(b"n");
                }
            }
        }
        put(b" ]");
    }
}
