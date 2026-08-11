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
//! `__torajs_io_putc_out` byte writers instead.

use core::ffi::c_void;

use super::formatters::{
    __torajs_anyv_struct_print_inline_at, __torajs_arr_print_any_at, __torajs_bigint_print_inline,
    __torajs_inspect_line_add, __torajs_map_print_at, __torajs_obj_print_any_at,
    __torajs_promise_print, __torajs_regex_print_inline, __torajs_set_print_at,
    __torajs_symbol_print_inline, SUBSTR_VIEW_FLAG, heap_flags, heap_type_tag, put_byte, put_bytes,
    put_closure_fn_name, put_cp_json_escaped, put_date_inline, put_f64_inline, put_i64_inline,
    put_str_cell_inline_esc, put_substr_cell_inline_esc,
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
    unsafe { __torajs_print_anyv_inline_at(v, 0) }
}

/// Indent-threaded variant of [`__torajs_print_anyv_inline`] —
/// inspect indent trunk. `indent` is the *composite's own* indent
/// column (bun's uniform model, probed 2026-07-05: every container
/// level adds 2, whether or not the outer container broke to
/// multi-line). Leaf arms ignore it; Tag::Arr / Tag::DynObj /
/// Tag::Obj thread it into their indent-aware walkers so nested
/// objects pad fields at `indent + 2` and the closer at `indent`.
///
/// # Safety
///
/// Same contract as [`__torajs_print_anyv_inline`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_print_anyv_inline_at(v: AnyValue, indent: u32) {
    if is_null(v) {
        unsafe { put_bytes(b"null") };
        __torajs_inspect_line_add(4);
        return;
    }
    if is_undefined(v) {
        unsafe { put_bytes(b"undefined") };
        __torajs_inspect_line_add(9);
        return;
    }
    if is_bool(v) {
        let b = as_bool(v);
        unsafe { put_bytes(if b { b"true" } else { b"false" }) };
        __torajs_inspect_line_add(if b { 4 } else { 5 });
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
        // Nested context — strings get `"..."` quotes with JSON
        // escapes (bun routes quoted inspect strings through
        // writeJSONString). Top-level `__torajs_print_anyv` skips
        // both (matches bun's `console.log("hi")` → `hi`).
        let len = short_str_len(v) as usize;
        let bytes = short_str_bytes(v);
        unsafe { put_byte(b'"') };
        for &b in &bytes[..len] {
            if unsafe { put_cp_json_escaped(b as u32) } {
                continue;
            }
            unsafe { put_byte(b) };
        }
        unsafe { put_byte(b'"') };
        // Quote-free width accounting (bun parity — see
        // put_str_cell_inline).
        __torajs_inspect_line_add(len as u32);
        return;
    }
    if is_cell(v) {
        let child = as_void_ptr(v) as *const c_void;
        // SAFETY: live heap ptr per caller invariant.
        let tag = unsafe { heap_type_tag(child) };
        if tag == Tag::Undefined as u16 {
            // RFC 20260722 chunk C — the generic undefined sentinel
            // cell (typed-lane composite slot: `[find-miss]` /
            // optional struct field) prints as the value it
            // encodes, not the [object] fallback.
            unsafe { put_bytes(b"undefined") };
            __torajs_inspect_line_add(9);
        } else if tag == Tag::Str as u16 {
            // Substr vs real Str — flag bit 10 disambiguates same as
            // the standalone print_anyv path above. Nested context
            // → `"..."` quotes (see ShortStr arm above for rationale
            // and escape caveat).
            let flags = unsafe { heap_flags(child) };
            unsafe { put_byte(b'"') };
            if flags & SUBSTR_VIEW_FLAG != 0 {
                unsafe { put_substr_cell_inline_esc(child, true) };
            } else {
                unsafe { put_str_cell_inline_esc(child, true) };
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
            unsafe { __torajs_arr_print_any_at(child, indent) };
        } else if tag == Tag::DynObj as u16 {
            // Commit 4 — Tag::DynObj nested. Same recursion form
            // as Tag::Arr above.
            // SAFETY: dynobj layout per torajs-dynobj::layout.
            unsafe { __torajs_obj_print_any_at(child, indent) };
        } else if tag == Tag::Date as u16 {
            // Commit 5 — nested Date prints unquoted (bun:
            // `[ 1970-01-01T00:00:00.000Z ]` not
            // `[ "1970-..." ]`), or `Invalid Date` for the invalid
            // sentinel; no trailing '\n'.
            // SAFETY: Date layout per torajs-date::layout.
            unsafe { put_date_inline(child) };
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
            // closure print, method-cell name aware (chunk 715);
            // the trailing '\n' is owned by the nested-format
            // outer walker.
            unsafe { put_closure_fn_name(child) };
        } else if tag == Tag::Map as u16 {
            // Runtime Tag::Set substrate — nested Map cell prints
            // inline (`Map {}` / `Map(N) {…}` form, no trailing
            // '\n'), rows padded at this cell's indent + 2.
            // SAFETY: Map layout per torajs-collections::layout.
            unsafe { __torajs_map_print_at(child, indent) };
        } else if tag == Tag::Set as u16 {
            // SAFETY: Set shares Map layout, just stamped TAG_SET.
            unsafe { __torajs_set_print_at(child, indent) };
        } else if tag == Tag::Obj as u16 {
            // W-J Phase D — nested Tag::Obj struct cell prints the
            // same `Name {…}` form as top-level (no trailing '\n';
            // outer walker owns separators).
            unsafe { __torajs_anyv_struct_print_inline_at(v, indent) };
        } else if tag == Tag::WeakMap as u16 {
            // WeakMap / WeakSet are non-enumerable per spec
            // (§24.4 / §24.5 — no `forEach`, no iterators), so
            // bun's default inspect prints the fixed `WeakMap {}` /
            // `WeakSet {}` form regardless of entry count. No
            // trailing '\n' (inline contract).
            unsafe { put_bytes(b"WeakMap {}") };
        } else if tag == Tag::WeakSet as u16 {
            unsafe { put_bytes(b"WeakSet {}") };
        } else if tag == Tag::Symbol as u16 {
            // Nested-context Symbol — `Symbol(<desc>)` form (no
            // trailing '\n'; outer walker owns separators). Same
            // bytes the top-level `__torajs_symbol_print` emits,
            // minus the '\n'.
            unsafe { __torajs_symbol_print_inline(child) };
        } else if tag == Tag::BigInt as u16 {
            // Nested-context BigInt — `<decimal>n` form (no
            // trailing '\n'). Allocates a temporary decimal Str
            // via `__torajs_bigint_to_string`, emits its bytes,
            // appends the literal `n` suffix per JS BigInt
            // notation.
            unsafe { __torajs_bigint_print_inline(child) };
        } else if tag == Tag::StringWrapper as u16 {
            // Nested-context String wrapper — bun prints just the
            // quoted [[StringData]] (`[ "a" ]`), unlike the
            // top-level `[String: "a"]` form. Inner Str cell @ +8;
            // NULL is the empty-string sentinel.
            unsafe {
                put_byte(b'"');
                let inner = ((child as *const u8).add(8) as *const *const c_void).read();
                if !inner.is_null() {
                    put_str_cell_inline_esc(inner, true);
                }
                put_byte(b'"');
            }
        } else if tag == Tag::SymbolWrapper as u16 {
            // Object(sym) — fixed four-field multi-line block
            // (rotation 184; fields at indent + 2).
            unsafe { crate::inspect::wrapper_block::put_symbol_wrapper_at(child, indent) };
        } else if unsafe { crate::inspect::formatters::put_wrapper_inline(child, tag) } {
            // Primitive wrapper — bytes emitted by the helper.
        } else {
            // All other composite / typed-receiver tags
            // (Tag::Symbol / Tag::BigInt / Tag::Response /
            // Tag::WeakRef / Tag::MapIter / Tag::ArrIter /
            // Tag::AccessorPair / etc) fall back to `[object]`
            // (no '\n'). Wire each into its typed walker as the
            // corresponding substrate lands.
            unsafe { put_bytes(b"[object]") };
        }
        return;
    }
    unsafe { put_bytes(b"[unknown-any-tag]") };
}
