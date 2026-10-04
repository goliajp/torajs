//! The `space` argument of `JSON.stringify` at run time: the indent a
//! gap makes, the `:` it widens, and the closing-bracket indent. Each
//! is called from the compiler's static unfold only when the call site
//! carries a `space`.

use crate::block::StrBlock;
use crate::eq::resolve_payload;
use crate::layout::STR_LEN_OFF;
use crate::max_len::{__torajs_str_too_long, str_len_of};

/// ES §25.5.2.1 — one indent step for the static unfold: a newline
/// followed by `depth` copies of `gap`, in the gap's own encoding (a
/// UTF-16 gap makes a UTF-16 indent; the pre-560 form copied `len`
/// payload bytes into a Latin-1 block, half of such a gap). The
/// compile-time walk only emits a call to this when the call site
/// actually carries a `space` argument, so the compact form pays
/// nothing.
///
/// # Safety
///
/// `gap` is a live Str block (or NULL for the empty gap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_indent(gap: *const u8, depth: i64) -> *mut u8 {
    let (payload, latin1) = if gap.is_null() {
        (&[][..], true)
    } else {
        unsafe { resolve_payload(gap) }
    };
    if payload.is_empty() {
        // An empty gap is the compact form (a Number space of 0, an
        // empty String space): §25.5.2.4 puts no line break in at
        // all, so this is the empty string rather than a bare "\n".
        return StrBlock::alloc(0).into_raw();
    }
    let stride = if latin1 { 1 } else { 2 };
    let depth = depth.max(0) as usize;
    let Some(out_units) = (payload.len() / stride)
        .checked_mul(depth)
        .and_then(|n| str_len_of(n + 1))
    else {
        return unsafe { __torajs_str_too_long() };
    };
    let mut block = StrBlock::alloc_with_encoding(out_units, latin1);
    // SAFETY: block was just allocated with `out_units` units of capacity.
    let dst = unsafe { block.as_bytes_mut((out_units as usize * stride) as u32) };
    dst[..stride].copy_from_slice(&[b'\n', 0][..stride]);
    for i in 0..depth {
        let at = stride + i * payload.len();
        dst[at..at + payload.len()].copy_from_slice(payload);
    }
    block.into_raw()
}

/// The `:` between a property key and its value — ES §25.5.2.4 step
/// 9.b.iii gives it a trailing space only under a NON-EMPTY gap, and
/// whether the gap is empty is a runtime fact (a Number space of 0
/// normalizes to one), so the static unfold asks here instead of
/// interning one of the two spellings at compile time.
///
/// # Safety
///
/// `gap` is a live Str block (or NULL for the empty gap).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_colon(gap: *const u8) -> *mut u8 {
    let gap_len = if gap.is_null() {
        0
    } else {
        unsafe { (gap.add(STR_LEN_OFF) as *const u32).read() }
    };
    let text: &[u8] = if gap_len == 0 { b":" } else { b": " };
    let mut block = StrBlock::alloc(text.len() as u32);
    // SAFETY: block was just allocated with `text.len()` payload bytes.
    let dst = unsafe { block.as_bytes_mut(text.len() as u32) };
    dst.copy_from_slice(text);
    block.into_raw()
}

/// The closing-bracket indent of a composite under a gap: the
/// bracket returns to the PARENT's level, but only when the body
/// emitted something — `[]` and `{}` stay on one line. "Emitted
/// something" is the same accumulator-length test
/// [`crate::json::__torajs_json_obj_sep`] uses, since `acc` still holds just its
/// opening bracket otherwise.
///
/// Owned-in, owned-out (642-ledger): the caller hands over its
/// accumulator stake and receives one back.
///
/// # Safety
///
/// `acc` is a live Str block whose stake the caller relinquishes;
/// `gap` is a live Str block (or NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_close_indent(
    acc: *mut u8,
    gap: *const u8,
    depth: i64,
) -> *mut u8 {
    let gap_len = if gap.is_null() {
        0
    } else {
        unsafe { (gap.add(STR_LEN_OFF) as *const u32).read() }
    };
    let len = unsafe { (acc.add(STR_LEN_OFF) as *const u32).read() };
    if len <= 1 || gap_len == 0 {
        return acc;
    }
    let indent = unsafe { __torajs_json_indent(gap, depth) };
    let out = unsafe { crate::concat::__torajs_str_concat(acc, indent) };
    unsafe { crate::__torajs_str_drop(indent) };
    unsafe { crate::__torajs_str_drop(acc) };
    out
}
