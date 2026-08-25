//! Any-lane `JSON.parse` — parse a complete JSON text into a
//! NaN-boxed value tree (RFC 20260808-json-parse-any blade 1).
//!
//! The caller-typed lane (`ssa_lower_json_parse.rs`) drives a static
//! per-shape parse and stays the fast path; this kernel serves the
//! sites with no caller type — an expression-position
//! `JSON.parse(text).a`, an `any`-annotated target, and (blade 3)
//! every 2-arg reviver call, whose result shape a reviver can rewrite
//! arbitrarily.
//!
//! Value mapping per §25.5.1 / ECMA-404: number → f64 box (JSON has
//! one number type), string → Str cell, `true`/`false`/`null` →
//! immediates, array → `Arr<Any>`, object → dynobj whose entries are
//! CreateDataProperty stores (plain own-slot writes — `__proto__` is
//! an ordinary key here, and `__torajs_dynobj_set` writes the entry
//! table, never a prototype). Scalar tokens ride the existing
//! `torajs-str` token helpers (`__torajs_json_parse_float` /
//! `_string`), which record a pending SyntaxError-shaped throw on
//! malformed input; structural errors here go through
//! `__torajs_throw_syntax_error`. On any pending throw the partial
//! tree is released and `undefined` returns — the compiler emits its
//! usual throw_check after the call.

use core::ffi::c_void;

use crate::compare::STR_FLAG_IS_LATIN1;
use crate::member_set::{STR_DATA_OFF, STR_LEN_OFF};
use crate::nanbox::{AnyValue, VALUE_FALSE, VALUE_NULL, VALUE_TRUE, VALUE_UNDEFINED};
use crate::nanbox_encode::{__torajs_anyv_box_f64, __torajs_anyv_box_from_pair};
use torajs_rc::HeapHeader;

/// `nanbox_encode` pair-encoding heap tag (0=Null 1=Bool 2=I64 3=F64
/// 4=Heap 5=Undef).
const ANY_HEAP: u64 = 4;

/// Nesting bound — the typed lane's depth is capped by the static
/// type it parses into; this lane recurses on runtime data, so a
/// hostile `[[[[…` must fail loudly instead of blowing the stack.
const MAX_DEPTH: u32 = 512;

unsafe extern "C" {
    fn __torajs_throw_syntax_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;

    /// torajs-str JSON token helpers — full number grammar (sign,
    /// fraction, exponent) / string with escapes; each advances the
    /// cursor and records a pending throw on malformed input.
    fn __torajs_json_parse_float(str_ptr: *const u8, pos: *mut i64) -> f64;
    fn __torajs_json_parse_string(str_ptr: *const u8, pos: *mut i64) -> *mut u8;

    fn __torajs_str_drop(s: *mut c_void);

    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set_fresh(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );

    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    /// May reallocate — always continue with the returned pointer.
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
}

/// `JSON.parse(text)` with no caller type — returns the boxed value
/// tree, or `undefined` with a pending throw recorded.
///
/// # Safety
/// `text` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_parse_any(text: AnyValue) -> AnyValue {
    unsafe {
        // §25.5.1 step 1 — ToString(text); the Symbol arm records the
        // TypeError and answers a placeholder Str to drop.
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(text);
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(s);
            return VALUE_UNDEFINED;
        }
        let mut pos = 0i64;
        let v = parse_value(s.cast::<u8>(), &mut pos, 0);
        if __torajs_throw_check() != 0 {
            release(v);
            __torajs_str_drop(s);
            return VALUE_UNDEFINED;
        }
        // A complete JSON text has nothing but whitespace after the
        // value (`ECMA-404`); `{"a":1} x` must throw.
        let data = str_units(s.cast::<u8>());
        skip_ws(&data, &mut pos);
        if (pos as usize) < data.len() {
            __torajs_throw_syntax_error(c"JSON.parse: unexpected trailing characters".as_ptr());
            release(v);
            __torajs_str_drop(s);
            return VALUE_UNDEFINED;
        }
        __torajs_str_drop(s);
        v
    }
}

/// The JSON text addressed by **code unit**, which is what the
/// grammar is written over. Every unit that shapes it is ASCII, so
/// this view answers "the ASCII unit at `i`" and lets everything
/// else — string content — reach the token helper untouched.
///
/// Post-P11.1-S2 a Str payload is Latin-1 (one byte per unit) or
/// UTF-16 LE (two), with `length` counting units either way. Reading
/// `length` bytes and calling them the text handed a UTF-16 source
/// half of its own payload: `JSON.parse(JSON.stringify(["中"]))`
/// threw SyntaxError on the opening bracket.
struct JsonUnits {
    base: *const u8,
    len: usize,
    latin1: bool,
}

impl JsonUnits {
    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    /// The unit at `i` when it is in range and ASCII. `None` covers
    /// both "past the end" and "not a byte any token is made of".
    #[inline]
    fn ascii(&self, i: usize) -> Option<u8> {
        if i >= self.len {
            return None;
        }
        let u = unsafe {
            if self.latin1 {
                *self.base.add(i) as u16
            } else {
                u16::from_le_bytes([*self.base.add(i * 2), *self.base.add(i * 2 + 1)])
            }
        };
        if u < 0x80 { Some(u as u8) } else { None }
    }

    /// Whether the units from `i` spell the ASCII `kw`.
    fn keyword(&self, i: usize, kw: &[u8]) -> bool {
        kw.iter()
            .enumerate()
            .all(|(k, &b)| self.ascii(i + k) == Some(b))
    }
}

/// View a Str pointer's payload as JSON text.
///
/// # Safety
/// `str_ptr` must be a live Str heap block outliving the view.
unsafe fn str_units(str_ptr: *const u8) -> JsonUnits {
    unsafe {
        JsonUnits {
            base: str_ptr.add(STR_DATA_OFF),
            len: (str_ptr.add(STR_LEN_OFF) as *const u32).read() as usize,
            latin1: (*(str_ptr as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0,
        }
    }
}

fn skip_ws(data: &JsonUnits, pos: &mut i64) {
    while (*pos as usize) < data.len() {
        match data.ascii(*pos as usize) {
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') => *pos += 1,
            _ => break,
        }
    }
}

/// Release an owned boxed value (partial-tree cleanup on the error
/// paths; immediates are no-ops inside `rc_dec`).
unsafe fn release(v: AnyValue) {
    unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(v) }
}

/// One JSON value at `*pos` — dispatches on the first non-ws byte.
/// Returns the OWNED box; on error records the throw and returns
/// `undefined` (callers check the throw flag, not the value).
unsafe fn parse_value(str_ptr: *const u8, pos: &mut i64, depth: u32) -> AnyValue {
    unsafe {
        if depth > MAX_DEPTH {
            __torajs_throw_syntax_error(c"JSON.parse: structure too deep".as_ptr());
            return VALUE_UNDEFINED;
        }
        let data = str_units(str_ptr);
        skip_ws(&data, pos);
        if *pos as usize >= data.len() {
            __torajs_throw_syntax_error(c"JSON.parse: unexpected end of input".as_ptr());
            return VALUE_UNDEFINED;
        }
        match data.ascii(*pos as usize).unwrap_or(0) {
            b'{' => parse_object(str_ptr, pos, depth),
            b'[' => parse_array(str_ptr, pos, depth),
            b'"' => {
                let cell = __torajs_json_parse_string(str_ptr, pos);
                if __torajs_throw_check() != 0 {
                    __torajs_str_drop(cell.cast());
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_from_pair(ANY_HEAP as i64, cell as i64)
            }
            b't' | b'f' => {
                if data.keyword(*pos as usize, b"true") {
                    *pos += 4;
                    VALUE_TRUE
                } else if data.keyword(*pos as usize, b"false") {
                    *pos += 5;
                    VALUE_FALSE
                } else {
                    __torajs_throw_syntax_error(c"JSON.parse: unexpected token".as_ptr());
                    VALUE_UNDEFINED
                }
            }
            b'n' => {
                if data.keyword(*pos as usize, b"null") {
                    *pos += 4;
                    VALUE_NULL
                } else {
                    __torajs_throw_syntax_error(c"JSON.parse: unexpected token".as_ptr());
                    VALUE_UNDEFINED
                }
            }
            b'-' | b'0'..=b'9' => {
                let x = __torajs_json_parse_float(str_ptr, pos);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                __torajs_anyv_box_f64(x)
            }
            _ => {
                __torajs_throw_syntax_error(c"JSON.parse: unexpected token".as_ptr());
                VALUE_UNDEFINED
            }
        }
    }
}

unsafe fn parse_array(str_ptr: *const u8, pos: &mut i64, depth: u32) -> AnyValue {
    unsafe {
        *pos += 1; // consume '['
        let mut arr = __torajs_arr_alloc_any(0) as *mut c_void;
        let data = str_units(str_ptr);
        skip_ws(&data, pos);
        if data.ascii(*pos as usize) == Some(b']') {
            *pos += 1;
            return __torajs_anyv_box_from_pair(ANY_HEAP as i64, arr as i64);
        }
        loop {
            let elem = parse_value(str_ptr, pos, depth + 1);
            if __torajs_throw_check() != 0 {
                release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, arr as i64));
                return VALUE_UNDEFINED;
            }
            let (t, p) = (
                crate::__torajs_anyv_unbox_tag(elem),
                crate::__torajs_anyv_unbox_value(elem),
            );
            arr = __torajs_arr_push_any(arr, t as u64, p as u64) as *mut c_void;
            let data = str_units(str_ptr);
            skip_ws(&data, pos);
            match data.ascii(*pos as usize) {
                Some(b',') => *pos += 1,
                Some(b']') => {
                    *pos += 1;
                    return __torajs_anyv_box_from_pair(ANY_HEAP as i64, arr as i64);
                }
                _ => {
                    __torajs_throw_syntax_error(c"JSON.parse: expected ',' or ']'".as_ptr());
                    release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, arr as i64));
                    return VALUE_UNDEFINED;
                }
            }
        }
    }
}

unsafe fn parse_object(str_ptr: *const u8, pos: &mut i64, depth: u32) -> AnyValue {
    unsafe {
        *pos += 1; // consume '{'
        let mut obj = __torajs_dynobj_alloc();
        let data = str_units(str_ptr);
        skip_ws(&data, pos);
        if data.ascii(*pos as usize) == Some(b'}') {
            *pos += 1;
            return __torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64);
        }
        loop {
            let data = str_units(str_ptr);
            skip_ws(&data, pos);
            if data.ascii(*pos as usize) != Some(b'"') {
                __torajs_throw_syntax_error(c"JSON.parse: expected string key".as_ptr());
                release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64));
                return VALUE_UNDEFINED;
            }
            let key = __torajs_json_parse_string(str_ptr, pos);
            if __torajs_throw_check() != 0 {
                __torajs_str_drop(key.cast());
                release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64));
                return VALUE_UNDEFINED;
            }
            let data = str_units(str_ptr);
            skip_ws(&data, pos);
            if data.ascii(*pos as usize) != Some(b':') {
                __torajs_throw_syntax_error(c"JSON.parse: expected ':'".as_ptr());
                __torajs_str_drop(key.cast());
                release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64));
                return VALUE_UNDEFINED;
            }
            *pos += 1;
            let val = parse_value(str_ptr, pos, depth + 1);
            if __torajs_throw_check() != 0 {
                __torajs_str_drop(key.cast());
                release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64));
                return VALUE_UNDEFINED;
            }
            let (t, p) = (
                crate::__torajs_anyv_unbox_tag(val),
                crate::__torajs_anyv_unbox_value(val),
            );
            // Duplicate keys: last one wins (§25.5.1 — the entry
            // overwrite drops the earlier value inside dynobj_set).
            __torajs_dynobj_set_fresh(&mut obj, key.cast(), t as u64, p as u64);
            __torajs_str_drop(key.cast());
            let data = str_units(str_ptr);
            skip_ws(&data, pos);
            match data.ascii(*pos as usize) {
                Some(b',') => *pos += 1,
                Some(b'}') => {
                    *pos += 1;
                    return __torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64);
                }
                _ => {
                    __torajs_throw_syntax_error(c"JSON.parse: expected ',' or '}'".as_ptr());
                    release(__torajs_anyv_box_from_pair(ANY_HEAP as i64, obj as i64));
                    return VALUE_UNDEFINED;
                }
            }
        }
    }
}
