//! Substr-involved concat FFI shims — three permutations of
//! `(Substr + Str)` / `(Str + Substr)` / `(Substr + Substr)`, all
//! routed through `build_concat_result` for encoding-aware single-
//! alloc concatenation. Extracted from `substr_methods.rs` to keep
//! that file under the 500-prod-LOC file-size hard limit (`rules/
//! common/file-size.md`). Pure mechanical pull, no semantic change.

use core::ffi::c_void;

use crate::block::StrBlock;
use crate::substr_methods::{str_view, substr_view};

/// Build a result Str holding `(a_payload, a_latin1)` followed by
/// `(b_payload, b_latin1)` under the widest-of-inputs encoding,
/// widening the Latin-1 side to UTF-16 LE when the encodings
/// disagree. Mirrors `concat.rs::__torajs_str_concat` for the
/// `(substr, str)` / `(str, substr)` / `(substr, substr)` mixes.
fn build_concat_result(
    a_payload: &[u8],
    a_latin1: bool,
    b_payload: &[u8],
    b_latin1: bool,
) -> *mut c_void {
    let out_latin1 = a_latin1 && b_latin1;
    let stride: u32 = if out_latin1 { 1 } else { 2 };
    let a_byte_cnt = if a_latin1 == out_latin1 {
        a_payload.len()
    } else {
        a_payload.len() * 2
    };
    let b_byte_cnt = if b_latin1 == out_latin1 {
        b_payload.len()
    } else {
        b_payload.len() * 2
    };
    let total_byte_cnt = a_byte_cnt + b_byte_cnt;
    let length = (total_byte_cnt as u32) / stride;
    let mut block = StrBlock::alloc_with_encoding(length, out_latin1);
    if total_byte_cnt == 0 {
        return block.into_raw() as *mut c_void;
    }
    let dst = unsafe { block.as_bytes_mut(total_byte_cnt as u32) };
    if !a_payload.is_empty() {
        if a_latin1 == out_latin1 {
            dst[..a_byte_cnt].copy_from_slice(a_payload);
        } else {
            // a is Latin-1, out is UTF-16 — widen.
            for (i, &b) in a_payload.iter().enumerate() {
                dst[i * 2] = b;
                dst[i * 2 + 1] = 0;
            }
        }
    }
    if !b_payload.is_empty() {
        let b_slot = &mut dst[a_byte_cnt..a_byte_cnt + b_byte_cnt];
        if b_latin1 == out_latin1 {
            b_slot.copy_from_slice(b_payload);
        } else {
            for (i, &b) in b_payload.iter().enumerate() {
                b_slot[i * 2] = b;
                b_slot[i * 2 + 1] = 0;
            }
        }
    }
    block.into_raw() as *mut c_void
}

/// `(substr + str)` — single-alloc view-aware concat.
///
/// P11.1-S2.5 Round 2 — encoding-aware via `build_concat_result`.
///
/// # Safety
/// `v` is a live `*const Substr`, `s` is a live `*const Str`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_str(
    v: *const u8,
    s: *const u8,
) -> *mut c_void {
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    build_concat_result(v_payload, v_latin1, s_payload, s_latin1)
}

/// `(str + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `s` is a live `*const Str`, `v` is a live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_str_substr(
    s: *const u8,
    v: *const u8,
) -> *mut c_void {
    let (s_payload, _, s_latin1) = unsafe { str_view(s) };
    let (v_payload, _, v_latin1) = unsafe { substr_view(v) };
    build_concat_result(s_payload, s_latin1, v_payload, v_latin1)
}

/// `(substr + substr)` — single-alloc view-aware concat.
///
/// # Safety
/// `a` and `b` are live `*const Substr`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_substr_concat_substr_substr(
    a: *const u8,
    b: *const u8,
) -> *mut c_void {
    let (a_payload, _, a_latin1) = unsafe { substr_view(a) };
    let (b_payload, _, b_latin1) = unsafe { substr_view(b) };
    build_concat_result(a_payload, a_latin1, b_payload, b_latin1)
}
