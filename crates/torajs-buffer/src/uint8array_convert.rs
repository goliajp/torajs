//! `Uint8Array.prototype`'s four text conversions — §23.2.3's
//! `toBase64` / `toHex` / `setFromBase64` / `setFromHex`. The codec
//! itself is next door in [`crate::uint8array_codec`]; this file is
//! the ES side of it: the brand check, the options bag, and the
//! result shapes.
//!
//! Three orderings the spec is deliberate about, and each is
//! observable from user code:
//!
//! - **Brand before options.** The receiver has to be a `Uint8Array`
//!   before anything reads the bag, so a getter in the bag never runs
//!   for a wrong receiver.
//! - **Extent after options.** A getter is user code and can detach
//!   the buffer, so the byte range is taken again once the bag has
//!   been read — never held across it.
//! - **Write, then throw.** `setFrom*` writes every byte it decoded
//!   before the error and only then raises it, which is what
//!   test262's `writes-up-to-error.js` measures.
//!
//! These are `Uint8Array`-only by §23.2: the other ten element types
//! do not get them, and the brand check here is what says so.

use core::ffi::{c_char, c_void};

use torajs_anyvalue::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, box_double, box_void_ptr, is_cell, is_short_str,
    short_str_bytes, short_str_len,
};
use torajs_rc::Tag;
use torajs_rc::str_wtf8::StrWtf8;

use crate::typedarray::{Kind, is_object_value};
use crate::typedarray_span::{Span, validate};
use crate::uint8array_codec::{
    Decoded, LastChunk, decode_base64, decode_hex, encode_base64, encode_hex,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_throw_syntax_error(msg: *const c_char);
    fn __torajs_throw_check() -> i64;
    fn __torajs_any_member_get_with_receiver(
        target: AnyValue,
        key: *const c_void,
        receiver: AnyValue,
    ) -> AnyValue;
    fn __torajs_anyv_rc_dec(v: AnyValue);
    fn __torajs_anyv_to_bool(v: AnyValue) -> bool;
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// A message with its NUL, for the two throw entries.
macro_rules! cmsg {
    ($s:literal) => {
        concat!($s, "\0").as_ptr() as *const c_char
    };
}

unsafe fn type_error(msg: *const c_char) -> AnyValue {
    unsafe { __torajs_throw_type_error(msg) };
    VALUE_UNDEFINED
}

/// §23.2 ValidateUint8Array — the brand alone. `validate` answers the
/// whole `%TypedArray%` family, and these four are `Uint8Array`'s.
unsafe fn uint8_span(recv: AnyValue) -> Option<Span> {
    let span = unsafe { validate(recv) }?;
    if span.kind != Kind::Uint8 {
        unsafe { __torajs_throw_type_error(cmsg!("this is not a Uint8Array")) };
        return None;
    }
    Some(span)
}

/// The argument has to BE a String — §23.2 says "is not a String,
/// throw", so nothing here coerces. `None` means it was not one.
unsafe fn string_bytes(v: AnyValue) -> Option<Vec<u8>> {
    if is_short_str(v) {
        let n = short_str_len(v) as usize;
        return Some(short_str_bytes(v)[..n].to_vec());
    }
    if is_cell(v) {
        let p = as_void_ptr(v);
        if unsafe { p.cast::<u8>().add(4).cast::<u16>().read() } == Tag::Str as u16 {
            return Some(unsafe { StrWtf8::of(p) }.as_bytes().to_vec());
        }
    }
    None
}

unsafe fn mint_str(bytes: &[u8]) -> AnyValue {
    let cell = unsafe { __torajs_str_alloc(bytes.as_ptr(), bytes.len() as i64) };
    box_void_ptr(cell as *mut c_void)
}

/// §23.2 GetOptionsObject. `undefined` is an empty bag; anything that
/// is not an Object is a TypeError. The empty bag is spelled
/// `VALUE_UNDEFINED` here and short-circuits every read below, which
/// is the same answer an object with no such properties gives.
unsafe fn options_bag(options: AnyValue) -> Option<AnyValue> {
    if options == VALUE_UNDEFINED {
        return Some(VALUE_UNDEFINED);
    }
    if is_object_value(options) {
        return Some(options);
    }
    unsafe { __torajs_throw_type_error(cmsg!("options is not an object")) };
    None
}

/// One [[Get]] off the bag, owned. `None` carries a pending throw.
unsafe fn bag_get(bag: AnyValue, name: &[u8]) -> Option<AnyValue> {
    if bag == VALUE_UNDEFINED {
        return Some(VALUE_UNDEFINED);
    }
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *const c_void;
        let got = __torajs_any_member_get_with_receiver(bag, key, bag);
        __torajs_str_drop(key as *mut c_void);
        if __torajs_throw_check() != 0 {
            return None;
        }
        Some(got)
    }
}

/// §23.2 GetOption for a string-valued option: absent takes the
/// default, a non-String is a TypeError, and a String outside the
/// list is one too. The answer is the index into `allowed`.
unsafe fn string_option(bag: AnyValue, name: &[u8], allowed: &[&[u8]]) -> Option<usize> {
    let got = unsafe { bag_get(bag, name) }?;
    if got == VALUE_UNDEFINED {
        return Some(0);
    }
    let bytes = unsafe { string_bytes(got) };
    unsafe { __torajs_anyv_rc_dec(got) };
    let Some(bytes) = bytes else {
        unsafe { __torajs_throw_type_error(cmsg!("option value is not a string")) };
        return None;
    };
    match allowed.iter().position(|a| *a == bytes.as_slice()) {
        Some(i) => Some(i),
        None => {
            unsafe {
                __torajs_throw_type_error(cmsg!("option value is not one of the allowed ones"))
            };
            None
        }
    }
}

/// §23.2 reads `omitPadding` through ToBoolean, so every value is
/// legal and `{ omitPadding: 0 }` keeps the padding.
unsafe fn bool_option(bag: AnyValue, name: &[u8]) -> Option<bool> {
    let got = unsafe { bag_get(bag, name) }?;
    let b = unsafe { __torajs_anyv_to_bool(got) };
    unsafe { __torajs_anyv_rc_dec(got) };
    Some(b)
}

const ALPHABETS: [&[u8]; 2] = [b"base64", b"base64url"];
const HANDLINGS: [&[u8]; 3] = [b"loose", b"strict", b"stop-before-partial"];

fn handling_of(i: usize) -> LastChunk {
    match i {
        1 => LastChunk::Strict,
        2 => LastChunk::StopBeforePartial,
        _ => LastChunk::Loose,
    }
}

/// # Safety
/// `recv` and `options` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_to_base64(
    recv: AnyValue,
    options: AnyValue,
) -> AnyValue {
    unsafe {
        if uint8_span(recv).is_none() {
            return VALUE_UNDEFINED;
        }
        let Some(bag) = options_bag(options) else {
            return VALUE_UNDEFINED;
        };
        let Some(alphabet) = string_option(bag, b"alphabet", &ALPHABETS) else {
            return VALUE_UNDEFINED;
        };
        let Some(omit) = bool_option(bag, b"omitPadding") else {
            return VALUE_UNDEFINED;
        };
        // The bag's getters ran, so the extent is asked for again.
        let Some(span) = uint8_span(recv) else {
            return VALUE_UNDEFINED;
        };
        let src = core::slice::from_raw_parts(span.base, span.len as usize);
        mint_str(&encode_base64(src, alphabet == 1, omit))
    }
}

/// # Safety
/// `recv` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_to_hex(recv: AnyValue) -> AnyValue {
    unsafe {
        let Some(span) = uint8_span(recv) else {
            return VALUE_UNDEFINED;
        };
        let src = core::slice::from_raw_parts(span.base, span.len as usize);
        mint_str(&encode_hex(src))
    }
}

/// `{ read, written }` — §23.2's result record for both `setFrom*`.
unsafe fn read_written(read: usize, written: usize) -> AnyValue {
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        for (name, n) in [(&b"read"[..], read), (&b"written"[..], written)] {
            let boxed = box_double(n as f64);
            let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
            __torajs_dynobj_set(
                &mut obj,
                key as *mut c_void,
                __torajs_anyv_unbox_tag(boxed) as u64,
                __torajs_anyv_unbox_value(boxed) as u64,
            );
            __torajs_str_drop(key as *mut c_void);
        }
        box_void_ptr(obj)
    }
}

/// Write the decoded prefix into the view, then raise the error if
/// there was one. `None` means the throw is pending.
unsafe fn commit(recv: AnyValue, d: Decoded) -> AnyValue {
    unsafe {
        // The decode ran no user code, so the extent taken before it
        // is still the one to write through — but ask again anyway,
        // because that is free and a stale base pointer is not.
        let Some(span) = uint8_span(recv) else {
            return VALUE_UNDEFINED;
        };
        let n = d.bytes.len().min(span.len as usize);
        core::ptr::copy_nonoverlapping(d.bytes.as_ptr(), span.base, n);
        if let Some(msg) = d.err {
            let mut owned: Vec<u8> = msg.as_bytes().to_vec();
            owned.push(0);
            __torajs_throw_syntax_error(owned.as_ptr() as *const c_char);
            return VALUE_UNDEFINED;
        }
        read_written(d.read, n)
    }
}

/// # Safety
/// `recv`, `string` and `options` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_set_from_base64(
    recv: AnyValue,
    string: AnyValue,
    options: AnyValue,
) -> AnyValue {
    unsafe {
        if uint8_span(recv).is_none() {
            return VALUE_UNDEFINED;
        }
        let Some(text) = string_bytes(string) else {
            return type_error(cmsg!("argument is not a string"));
        };
        let Some(bag) = options_bag(options) else {
            return VALUE_UNDEFINED;
        };
        let Some(alphabet) = string_option(bag, b"alphabet", &ALPHABETS) else {
            return VALUE_UNDEFINED;
        };
        let Some(handling) = string_option(bag, b"lastChunkHandling", &HANDLINGS) else {
            return VALUE_UNDEFINED;
        };
        let Some(span) = uint8_span(recv) else {
            return VALUE_UNDEFINED;
        };
        let d = decode_base64(
            &text,
            alphabet == 1,
            handling_of(handling),
            span.len as usize,
        );
        commit(recv, d)
    }
}

/// # Safety
/// `recv` and `string` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_set_from_hex(
    recv: AnyValue,
    string: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(span) = uint8_span(recv) else {
            return VALUE_UNDEFINED;
        };
        let Some(text) = string_bytes(string) else {
            return type_error(cmsg!("argument is not a string"));
        };
        let d = decode_hex(&text, span.len as usize);
        commit(recv, d)
    }
}

/// A fresh `Uint8Array` holding `bytes` — §23.2.2's two statics both
/// end here. `create_same_type` zero-fills a buffer of the right
/// length and answers a view over it, which is the same allocation
/// the constructor makes.
///
/// # Safety
/// Runs the allocator; answers `undefined` with a pending RangeError
/// when the length does not fit.
unsafe fn mint_uint8(bytes: &[u8]) -> AnyValue {
    unsafe {
        let out = crate::typedarray_ctor::create_same_type(Kind::Uint8, bytes.len() as i64);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let Some(span) = uint8_span(out) else {
            return VALUE_UNDEFINED;
        };
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), span.base, bytes.len());
        out
    }
}

/// §23.2.2.1 `Uint8Array.fromBase64(string, options)`. Unlike
/// `setFromBase64` there is no buffer to run out of, so nothing is
/// written on the error path — the whole answer is the new array or
/// the throw.
///
/// # Safety
/// `string` and `options` are live AnyValues.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_from_base64(
    string: AnyValue,
    options: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(text) = string_bytes(string) else {
            return type_error(cmsg!("argument is not a string"));
        };
        let Some(bag) = options_bag(options) else {
            return VALUE_UNDEFINED;
        };
        let Some(alphabet) = string_option(bag, b"alphabet", &ALPHABETS) else {
            return VALUE_UNDEFINED;
        };
        let Some(handling) = string_option(bag, b"lastChunkHandling", &HANDLINGS) else {
            return VALUE_UNDEFINED;
        };
        let d = decode_base64(&text, alphabet == 1, handling_of(handling), usize::MAX);
        if let Some(msg) = d.err {
            let mut owned: Vec<u8> = msg.as_bytes().to_vec();
            owned.push(0);
            __torajs_throw_syntax_error(owned.as_ptr() as *const c_char);
            return VALUE_UNDEFINED;
        }
        mint_uint8(&d.bytes)
    }
}

/// §23.2.2.2 `Uint8Array.fromHex(string)` — no options at all.
///
/// # Safety
/// `string` is a live AnyValue.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_uint8array_from_hex(string: AnyValue) -> AnyValue {
    unsafe {
        let Some(text) = string_bytes(string) else {
            return type_error(cmsg!("argument is not a string"));
        };
        let d = decode_hex(&text, usize::MAX);
        if let Some(msg) = d.err {
            let mut owned: Vec<u8> = msg.as_bytes().to_vec();
            owned.push(0);
            __torajs_throw_syntax_error(owned.as_ptr() as *const c_char);
            return VALUE_UNDEFINED;
        }
        mint_uint8(&d.bytes)
    }
}

/// The brand, by cell pointer, for the reflection faces in
/// torajs-anyvalue. Those answer "does this receiver have a method
/// called X" from the heap TAG, which is `TypedArray` for all eleven
/// element types — and these four are `Uint8Array`'s alone, so the
/// tag cannot decide it. Reading the kind belongs on this side: a
/// second copy of the cell layout over there is the drift this
/// crate's own doc warns about.
///
/// # Safety
/// `p` is a live TypedArray cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_typedarray_is_uint8(p: *const c_void) -> i64 {
    i64::from(unsafe { crate::typedarray::kind_of(p.cast_mut()) } == Kind::Uint8)
}
