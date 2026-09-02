//! `JSON.rawJSON(text)` / `JSON.isRawJSON(O)` — ES2026
//! json-parse-with-source (§25.5.1 / §25.5.2).
//!
//! `rawJSON` mints a frozen null-prototype object carrying the
//! `[[IsRawJSON]]` internal slot (header bit
//! `torajs_rc::FLAG_DYNOBJ_RAW_JSON`) and a single `"rawJSON"` own
//! data property holding the validated JSON text. The any-lane
//! `JSON.stringify` walk (`json_stringify.rs`) recognizes the bit
//! and splices the stored text verbatim — that is the whole point
//! of the API: embedding a number too precise for f64 (a BigInt's
//! digits) into JSON output without a round-trip through Number.
//!
//! §25.5.1 steps as implemented here:
//!
//! 1. `ToString(text)` — the coerce kernel's Symbol arm records the
//!    §7.1.17 TypeError.
//! 2. Empty string, or first/last code unit in {TAB, LF, CR,
//!    SPACE} → SyntaxError.
//! 3. Parse as a JSON text per ECMA-404; SyntaxError unless it is a
//!    complete scalar (string / number / `true` / `false` /
//!    `null`) — an object or array outermost value is explicitly
//!    rejected by the spec step itself.
//! 4-7. OrdinaryObjectCreate(null) + CreateDataPropertyOrThrow +
//!    SetIntegrityLevel(frozen).

use core::ffi::c_void;

use torajs_rc::{FLAG_DYNOBJ_RAW_JSON, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED, Tag};

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr, box_bool, box_void_ptr, is_cell};

/// torajs-anyvalue `ANY_HEAP` slot tag (`nanbox_encode` pair
/// encoding) — the dynobj entry stores the Str cell as a heap box.
const ANY_HEAP: u64 = 4;

unsafe extern "C" {
    fn __torajs_throw_syntax_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_check() -> i64;

    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);

    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    fn __torajs_dynobj_set_fresh(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    fn __torajs_dynobj_freeze_entries(obj: *mut c_void);
}

/// `JSON.rawJSON(text)` per §25.5.1. Returns the boxed frozen
/// rawJSON object, or `undefined` with a pending TypeError /
/// SyntaxError recorded.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_raw_json(v: AnyValue) -> AnyValue {
    unsafe {
        // Step 1 — ToString(text); the Symbol arm records the
        // TypeError and answers a placeholder Str to drop.
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(v);
        if __torajs_throw_check() != 0 {
            __torajs_str_drop(s);
            return VALUE_UNDEFINED;
        }
        // Steps 2-3 — surface-char gate + scalar JSON text grammar,
        // over the text's WTF-8 spelling (the payload is Latin-1 or
        // UTF-16 code units; the pre-560 byte read cut a UTF-16 text
        // in half and refused `'"😀"'`).
        let text = torajs_rc::str_wtf8::StrWtf8::of(s.cast());
        if !is_valid_raw_json(text.as_bytes()) {
            __torajs_str_drop(s);
            __torajs_throw_syntax_error(c"Invalid rawJSON value".as_ptr());
            return VALUE_UNDEFINED;
        }
        // Steps 4-7 — frozen null-proto carrier. Both the interned
        // key and the text Str transfer their +1 stakes into the
        // fresh entry; freeze clears writable + configurable while
        // keeping enumerable, and the header takes the integrity
        // triple plus the [[IsRawJSON]] bit.
        let mut obj = __torajs_dynobj_alloc();
        __torajs_dynobj_mark_null_proto(obj);
        let key = __torajs_str_alloc(c"rawJSON".as_ptr() as *const u8, 7);
        __torajs_dynobj_set_fresh(&mut obj, key as *mut c_void, ANY_HEAP, s as u64);
        __torajs_dynobj_freeze_entries(obj);
        let flags = obj.cast::<u8>().add(6) as *mut u16;
        *flags |= FLAG_FROZEN | FLAG_SEALED | FLAG_NON_EXTENSIBLE | FLAG_DYNOBJ_RAW_JSON;
        box_void_ptr(obj)
    }
}

/// `JSON.isRawJSON(O)` per §25.5.3 — true iff `O` carries the
/// `[[IsRawJSON]]` internal slot. Never throws.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_is_raw_json(v: AnyValue) -> AnyValue {
    unsafe {
        if !is_cell(v) {
            return box_bool(false);
        }
        let ptr = as_void_ptr(v);
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag != Tag::DynObj as u16 {
            return box_bool(false);
        }
        let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
        box_bool(flags & FLAG_DYNOBJ_RAW_JSON != 0)
    }
}

/// §25.5.1 steps 2-3 — the whole byte string must be one ECMA-404
/// scalar JSON value (string / number / `true` / `false` / `null`),
/// with no leading/trailing code units at all (step 2's TAB / LF /
/// CR / SPACE gate plus the "outermost object or array is a
/// SyntaxError" clause make any non-scalar surface invalid).
fn is_valid_raw_json(b: &[u8]) -> bool {
    if b.is_empty() || matches!(b[0], b'\t' | b'\n' | b'\r' | b' ') {
        return false;
    }
    if matches!(b[b.len() - 1], b'\t' | b'\n' | b'\r' | b' ') {
        return false;
    }
    match b[0] {
        b'"' => scan_json_string(b) == Some(b.len()),
        b'-' | b'0'..=b'9' => scan_json_number(b) == Some(b.len()),
        _ => b == b"true" || b == b"false" || b == b"null",
    }
}

/// Scan a complete JSON string literal starting at byte 0; answers
/// the end position (one past the closing quote) or None on any
/// grammar violation. Raw control characters (< 0x20) are invalid;
/// bytes ≥ 0x80 pass through (the text arrives as WTF-8).
fn scan_json_string(b: &[u8]) -> Option<usize> {
    let mut i = 1;
    while i < b.len() {
        match b[i] {
            b'"' => return Some(i + 1),
            b'\\' => {
                let esc = *b.get(i + 1)?;
                match esc {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => i += 2,
                    b'u' => {
                        if i + 6 > b.len() || !b[i + 2..i + 6].iter().all(u8::is_ascii_hexdigit) {
                            return None;
                        }
                        i += 6;
                    }
                    _ => return None,
                }
            }
            c if c < 0x20 => return None,
            _ => i += 1,
        }
    }
    None
}

/// Scan a complete JSON number starting at byte 0 (ECMA-404:
/// `-? (0 | [1-9][0-9]*) frac? exp?`); answers the end position or
/// None.
fn scan_json_number(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    if b.get(i) == Some(&b'-') {
        i += 1;
    }
    match b.get(i)? {
        b'0' => i += 1,
        b'1'..=b'9' => {
            while b.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return None,
    }
    if b.get(i) == Some(&b'.') {
        i += 1;
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if !b.get(i).is_some_and(u8::is_ascii_digit) {
            return None;
        }
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_literals_accepted() {
        for ok in [
            "1", "-1.1", "0.11", "1.1e-1", "11", "true", "false", "null", "\"foo\"",
        ] {
            assert!(is_valid_raw_json(ok.as_bytes()), "{ok}");
        }
    }

    #[test]
    fn surface_and_grammar_rejected() {
        for bad in [
            "",
            " 1",
            "1 ",
            "\t1",
            "1\n",
            "{}",
            "[]",
            "undefined",
            "01",
            "1.",
            "1e",
            "--1",
            "\"unterminated",
            "\"bad\\q\"",
            "\"ctl\u{1}\"",
            "truefalse",
            "1,2",
        ] {
            assert!(!is_valid_raw_json(bad.as_bytes()), "{bad:?}");
        }
    }

    #[test]
    fn unicode_escape_forms() {
        assert!(is_valid_raw_json(b"\"\\u0041\""));
        assert!(!is_valid_raw_json(b"\"\\u00\""));
        assert!(!is_valid_raw_json(b"\"\\uZZZZ\""));
    }
}
