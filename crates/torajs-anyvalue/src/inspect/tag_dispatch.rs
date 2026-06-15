//! Nested `console.log` dispatch — same NaN-box [`AnyValue`]
//! discriminant tree as [`super::any::__torajs_print_anyv`] but
//! every arm emits its bytes WITHOUT a trailing newline; used by
//! Commit 2+ composite walkers (`__torajs_arr_print_any`,
//! `__torajs_obj_print_any`, typed Map / Set printers, …) to format
//! AnyValue cells inside a larger composite.
//!
//! Cannot reuse the IR-emitted `print_i64 / f64 / bool` or
//! `__torajs_str_print` — those all emit their own '\n'. Drops to
//! 0-libc `__torajs_fmt_itoa` / `__torajs_fmt_dtoa` +
//! `__torajs_io_putc_stdout` byte writers instead.

use core::ffi::c_void;

use super::formatters::{
    __torajs_arr_print_any, __torajs_date_to_iso_string, __torajs_fn_print_inline,
    __torajs_map_print, __torajs_obj_print_any, __torajs_promise_print, __torajs_rc_dec,
    __torajs_regex_print_inline, __torajs_set_print, SUBSTR_VIEW_FLAG, closure_fn_addr, heap_flags,
    heap_type_tag, put_byte, put_bytes, put_f64_inline, put_i64_inline, put_str_cell_inline,
    put_substr_cell_inline,
};
use crate::nanbox::{
    AnyValue, as_bool, as_double, as_int32, as_void_ptr, is_bool, is_cell, is_double, is_int32,
    is_null, is_short_str, is_undefined, short_str_bytes, short_str_len,
};
use torajs_rc::Tag;

/// `console.log(...)` inline-print entry — same NaN-box dispatch
/// as [`super::any::__torajs_print_anyv`] but emits NO trailing
/// newline. Substrate for the nested-print Commit 2+ walkers
/// (`__torajs_arr_print_any` / `__torajs_obj_print_any` / typed
/// receiver helpers) — they call this fn on each cell of a
/// composite so it can be embedded mid-line.
///
/// Tag::Arr / Tag::Obj / Tag::Map / Tag::Set / Tag::Promise /
/// Tag::Date / Tag::RegExp / Tag::Closure / Tag::Symbol /
/// Tag::BigInt all degrade to `"[object]"` (without '\n') in this
/// Commit 1 scaffold. Commits 2-8 patch each into the proper
/// typed walker. The standalone `__torajs_print_anyv` above keeps
/// its current Tag::Str / Tag::Substr behaviour byte-equal — no
/// caller change yet.
///
/// # Safety
///
/// Cell case: encoded pointer must point to a valid heap object
/// matching its `HeapHeader::type_tag` layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv_inline(v: AnyValue) {
    if is_null(v) {
        unsafe { put_bytes(b"null") };
        return;
    }
    if is_undefined(v) {
        unsafe { put_bytes(b"undefined") };
        return;
    }
    if is_bool(v) {
        unsafe { put_bytes(if as_bool(v) { b"true" } else { b"false" }) };
        return;
    }
    if is_int32(v) {
        unsafe { put_i64_inline(as_int32(v) as i64) };
        return;
    }
    if is_double(v) {
        unsafe { put_f64_inline(as_double(v)) };
        return;
    }
    if is_short_str(v) {
        // Nested context — strings get `"..."` quotes (bun inspect
        // form, e.g. `[ "hi" ]` not `[ hi ]`). Top-level
        // `__torajs_print_anyv` skips the quoting (matches bun's
        // `console.log("hi")` → `hi`).  Escapes (`"`, `\`, control
        // chars) not yet honoured — pure-ASCII fixtures only for
        // this commit; full bun escape table is a later commit.
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        unsafe { put_byte(b'"') };
        for &b in &bytes[..len] {
            unsafe { put_byte(b) };
        }
        unsafe { put_byte(b'"') };
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Str as u16 {
            // Substr vs real Str — flag bit 10 disambiguates same as
            // the standalone print_anyv path above. Nested context
            // → `"..."` quotes (see ShortStr arm above for rationale
            // and escape caveat).
            let flags = unsafe { heap_flags(child) };
            unsafe { put_byte(b'"') };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                unsafe { put_substr_cell_inline(child) };
            } else {
                unsafe { put_str_cell_inline(child) };
            }
            unsafe { put_byte(b'"') };
        } else if tag == Tag::Arr as u16 {
            // Commit 4 — recursion enable. Nested Arr<Any> /
            // Arr<Arr<_>> walks via this branch (the outer
            // __torajs_arr_print_any calls
            // __torajs_print_anyv_inline on each slot; if that slot
            // is itself a Tag::Arr cell, we land here and recurse
            // back into __torajs_arr_print_any). No '\n' (inline
            // contract). Cyclic graphs trigger SO with no
            // `[Circular]` sentinel — known limitation tracked in
            // L3b (bun emits `[Circular *N]`, v0.7 doesn't).
            // SAFETY: Tag::Arr layout per torajs-arr::layout.
            unsafe { __torajs_arr_print_any(child) };
        } else if tag == Tag::DynObj as u16 {
            // Commit 4 — Tag::DynObj nested. Same recursion form
            // as Tag::Arr above.
            // SAFETY: dynobj layout per torajs-dynobj::layout.
            unsafe { __torajs_obj_print_any(child) };
        } else if tag == Tag::Date as u16 {
            // Commit 5 — nested Date prints unquoted (bun:
            // `[ 1970-01-01T00:00:00.000Z ]` not
            // `[ "1970-..." ]`). Same fresh-Str + rc_dec dance as
            // the top-level Tag::Date arm above, minus the
            // trailing '\n'.
            // SAFETY: Date layout per torajs-date::layout.
            let iso = unsafe { __torajs_date_to_iso_string(child) };
            if !iso.is_null() {
                unsafe { put_str_cell_inline(iso as *const c_void) };
                unsafe { __torajs_rc_dec(iso as *mut c_void) };
            }
        } else if tag == Tag::RegExp as u16 {
            // Commit 6 — nested RegExp prints unquoted same as
            // top-level (bun: `[ /abc/g ]` not `[ "/abc/g" ]`).
            // SAFETY: RegExp layout per torajs-regex::regex.
            unsafe { __torajs_regex_print_inline(child) };
        } else if tag == Tag::Promise as u16 {
            // Commit 8 — nested Promise prints same minimal form.
            // SAFETY: Promise layout per torajs-promise::layout.
            unsafe { __torajs_promise_print(child) };
        } else if tag == Tag::Closure as u16 {
            // Phase 2 wire (fn-name registry Step 5) — nested
            // closure print. Same table lookup as top-level
            // (Tag::Closure top-level arm above) but the trailing
            // '\n' is owned by the nested-format outer walker.
            let fn_addr = unsafe { closure_fn_addr(child) };
            unsafe { __torajs_fn_print_inline(fn_addr) };
        } else if tag == Tag::Map as u16 {
            // Runtime Tag::Set substrate — nested Map cell prints
            // inline (`Map {}` / `Map(N) {…}` form, no trailing '\n').
            // SAFETY: Map layout per torajs-collections::layout.
            unsafe { __torajs_map_print(child) };
        } else if tag == Tag::Set as u16 {
            // SAFETY: Set shares Map layout, just stamped TAG_SET.
            unsafe { __torajs_set_print(child) };
        } else {
            // All other composite / typed-receiver tags
            // (Tag::Obj / Tag::Symbol / Tag::BigInt / Tag::Response /
            // Tag::Weak* / Tag::MapIter / Tag::ArrIter /
            // Tag::AccessorPair / etc) fall back to `[object]`
            // (no '\n'). Wire each into its typed walker as the
            // corresponding substrate lands.
            unsafe { put_bytes(b"[object]") };
        }
        return;
    }
    unsafe { put_bytes(b"[unknown-any-tag]") };
}
