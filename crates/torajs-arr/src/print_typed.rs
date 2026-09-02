//! Shared typed-array pretty-print walker (inspect wrap trunk
//! chunk C) — one break/wrap engine for the five per-element-type
//! printer families in `print.rs` (trailing `'\n'`) and
//! `print_inline.rs` (no `'\n'`), plus the elem-kind dispatch inside
//! `print_any.rs`.
//!
//! Break/wrap heuristic mirrors bun 1.3.14
//! (ConsoleObject.zig:2410-2591) exactly like the Arr<Any> walker in
//! `print_any.rs`, with one simplification: typed elements are all
//! primitive, so the composite-first-element trigger can never fire —
//! the opener is full-break iff `len > 10` or the running line
//! estimate already exceeds 80 columns.
//!
//! Width accounting follows bun's deliberately-approximate model
//! (string quotes uncounted; digits / keyword literals / commas /
//! separator spaces counted) via the
//! `__torajs_inspect_line_{reset,add,len}` primitives hosted in
//! `torajs-anyvalue::inspect`.

use core::ffi::c_void;

use crate::layout::ARR_LEN_OFF;
use crate::print::{ARR_HEAD_OFF, put_byte, put_bytes, put_snprintf_f64_g, put_snprintf_i64};

unsafe extern "C" {
    /// Line-width estimate primitives (inspect wrap trunk) — hosted
    /// in `torajs-anyvalue::inspect::formatters`, resolved at
    /// `tr build` link time.
    fn __torajs_inspect_line_reset(cols: u32);
    fn __torajs_inspect_line_add(n: u32);
    fn __torajs_inspect_line_len() -> u32;
    /// Quoted + JSON-escaped Str / Substr cell printer (inspect
    /// escape trunk) — handles the Substr-view flag internally and
    /// accounts the quote-free payload width.
    fn __torajs_print_str_cell_quoted(cell: *const c_void);
    /// RFC 20260707 chunk 2 — the immortal `undefined` sentinel Str
    /// cell (torajs-str undef_sentinel.rs). An elem slot holding it
    /// prints bare `undefined`, never a quoted string.
    fn __torajs_str_undef() -> *mut u8;
}

/// Element kind selector for [`print_typed_at`] — one arm per
/// `__torajs_arr_print_*` family member.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum TypedKind {
    I64,
    F64,
    Bool,
    Str,
    Substr,
}

/// Emit one element and account its estimated width.
unsafe fn emit_elem(kind: TypedKind, slot: *const u8) {
    unsafe {
        match kind {
            TypedKind::I64 => {
                // put_snprintf_i64 accounts its own digit width.
                put_snprintf_i64(*(slot as *const i64));
            }
            TypedKind::F64 => {
                let v = *(slot as *const f64);
                if v.is_nan() {
                    put_bytes(b"NaN");
                    __torajs_inspect_line_add(3);
                } else if v == f64::INFINITY {
                    put_bytes(b"Infinity");
                    __torajs_inspect_line_add(8);
                } else if v == f64::NEG_INFINITY {
                    put_bytes(b"-Infinity");
                    __torajs_inspect_line_add(9);
                } else {
                    // Accounts its own width.
                    put_snprintf_f64_g(v);
                }
            }
            TypedKind::Bool => {
                let v = *(slot as *const i64);
                put_bytes(if v != 0 { b"true" } else { b"false" });
                __torajs_inspect_line_add(if v != 0 { 4 } else { 5 });
            }
            TypedKind::Str | TypedKind::Substr => {
                let s = *(slot as *const *const c_void);
                if s.is_null() || s == __torajs_str_undef() as *const c_void {
                    // Nullish slot — the undefined sentinel cell
                    // (non-participating regex capture, RFC 20260707
                    // chunk 2) or NULL — prints bare `undefined`,
                    // never the quoted payload.
                    put_bytes(b"undefined");
                    __torajs_inspect_line_add(9);
                    return;
                }
                // Quoted + JSON-escaped, Substr flag handled inside;
                // payload width accounted quote-free (bun parity).
                __torajs_print_str_cell_quoted(s);
            }
        }
    }
}

/// Indent-aware typed-array walker — bun break/wrap form, no
/// trailing newline. `indent` is this array's own indent column
/// (0 at top level; the Arr<Any> walker threads the nesting column
/// through its elem-kind dispatch).
///
/// # Safety
///
/// `arr` must be NULL or a valid `Arr` heap object whose slots
/// match `kind`'s layout (i64 / f64 / bool-as-i64 / `*Str` /
/// `*Substr`).
pub(crate) unsafe fn print_typed_at(arr: *const u8, indent: u32, kind: TypedKind) {
    unsafe {
        if arr.is_null() {
            put_bytes(b"null");
            return;
        }
        let len = *(arr.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            put_bytes(b"[]");
            __torajs_inspect_line_add(2);
            return;
        }
        let head = *(arr.add(ARR_HEAD_OFF) as *const u32);
        let my_indent = indent + 2;
        __torajs_inspect_line_add(2);
        // Typed elements are always primitive — only the length
        // threshold / prior overflow can force the full-break opener.
        let mut full = len > 10 || __torajs_inspect_line_len() > 80;
        if full {
            put_bytes(b"[\n");
            put_indent(my_indent);
            __torajs_inspect_line_reset(my_indent);
            __torajs_inspect_line_add(1);
        } else {
            put_bytes(b"[ ");
            __torajs_inspect_line_add(2);
        }
        for i in 0..len {
            if i > 0 {
                put_byte(b',');
                __torajs_inspect_line_add(1);
                if __torajs_inspect_line_len() > 80 {
                    put_byte(b'\n');
                    put_indent(my_indent);
                    __torajs_inspect_line_reset(my_indent);
                    full = true;
                } else {
                    put_byte(b' ');
                    __torajs_inspect_line_add(1);
                }
            }
            emit_elem(kind, crate::print::slot_addr(arr, head, i));
        }
        // Composite trailing `, key: value` props (regex exec
        // results etc). Unaccounted — prop-carrying arrays are
        // capture lists, far below the wrap threshold.
        crate::print_props::put_arrprops(arr as *mut c_void, my_indent);
        // Close on its own line ONLY when the opener broke or a
        // mid-loop wrap fired (probed 2026-07-05: bun keeps
        // `[ 7×"xxxxxxxxxx" ]` single-line at estimate 86 — the
        // close decision does NOT re-test the 80-column estimate).
        if full {
            __torajs_inspect_line_reset(indent);
            put_byte(b'\n');
            put_indent(indent);
            put_byte(b']');
        } else {
            put_bytes(b" ]");
            __torajs_inspect_line_add(2);
        }
    }
}

/// Top-level wrapper — fresh console.log line: reset the estimate,
/// walk at column 0. Shared by the ten thin `__torajs_arr_print_*`
/// / `*_inline` exports.
pub(crate) unsafe fn print_typed_top(arr: *const c_void, kind: TypedKind) {
    unsafe {
        __torajs_inspect_line_reset(0);
        print_typed_at(arr as *const u8, 0, kind);
    }
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}
