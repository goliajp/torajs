//! `console.log(arr: Array<Any>)` and `Array<Array<...>>` recursive
//! pretty-printer — nested-print substrate trunk Commit 2.
//!
//! Walks an `Arr` whose slots hold NaN-box `AnyValue`s (8 bytes
//! each, encoded as `i64` in the slot view) and emits each cell via
//! the inline `__torajs_print_anyv_inline` substrate from
//! `torajs-anyvalue::inspect`. Recursion happens **inside**
//! `__torajs_print_anyv_inline`'s Tag::Arr branch (wired in Commit 4),
//! so `__torajs_arr_print_any` itself stays flat and emits a
//! single-line `[ a, b, c ]` shape that matches bun for
//! `Array<Any>` / `Array<Array<_>>`.
//!
//! Output shape (bun-parity for these element types):
//! - `null` (no newline) for NULL arr (regex no-match arr never
//!   reaches Arr<Any>, but defensive same as `print::print_header`)
//! - `[]` (no newline) for empty arr
//! - `[ a, b, c ]` (no newline) for non-empty
//!
//! ## Newline policy
//!
//! Commit 2 emits **no trailing newline** — this helper is the
//! Tag::Arr branch of `__torajs_print_anyv_inline` (Commit 1) and the
//! arr-elem path of the SSA console.log dispatcher (Commit 4); both
//! callers add the '\n' themselves when the outer console.log call
//! terminates. The Commit 4 `__torajs_print_anyv` Tag::Arr arm
//! appends '\n' after this call returns.
//!
//! ## arrprops
//!
//! Composite trailing `, key: value` props (set on regex exec
//! results, etc) are NOT emitted here — those are the
//! `__torajs_arr_print_*` per-element-type printers' job
//! (`print_props::put_arrprops`), not the generic Any walker.
//! Arr<Any> arrays produced by Object.entries / spread / etc do not
//! carry arrprops by construction.

use core::ffi::c_void;

use crate::layout::{ARR_LEN_OFF, arr_data};
use crate::print::{put_byte, put_bytes};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// Indent-threaded inline AnyValue printer from
    /// `torajs-anyvalue::inspect` (inspect indent trunk).
    /// Cross-staticlib extern — resolved at `tr build` link time.
    /// Takes the raw 8 bytes of an `AnyValue` slot as `u64` plus the
    /// composite's own indent column and writes its bun-form
    /// rendering through `__torajs_io_putc_out` with no trailing
    /// newline.
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    /// Line-width estimate primitives (inspect wrap trunk) — mirror
    /// of bun's `estimated_line_length` accounting, hosted in
    /// `torajs-anyvalue::inspect::formatters`.
    fn __torajs_inspect_line_reset(cols: u32);
    fn __torajs_inspect_line_add(n: u32);
    fn __torajs_inspect_line_len() -> u32;
}

/// `console.log(arr: Array<Any>)` — inline (no trailing newline)
/// recursive walker. Each slot is treated as a NaN-box `AnyValue`
/// and dispatched through `__torajs_print_anyv_inline_at`.
///
/// # Safety
///
/// `arr` must be either NULL or a valid `Arr` heap object whose
/// slots hold NaN-box `AnyValue`s. Each slot is read as `i64` and
/// re-interpreted as `AnyValue` for the inline printer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_any(arr: *const c_void) {
    // Top-level entry (SSA dispatcher / any.rs top-level arm) — a
    // fresh console.log line starts at column 0.
    unsafe { __torajs_inspect_line_reset(0) };
    unsafe { print_any_at(arr, 0) }
}

/// Indent-threaded export of the walker (inspect indent trunk) —
/// `indent` is this array's own indent column. Cross-staticlib
/// callers: `torajs-anyvalue::inspect::tag_dispatch`'s Tag::Arr arm
/// threads the surrounding composite's element indent through here
/// so nested objects inside this array pad correctly (bun pads by
/// depth even when every enclosing array stays single-line).
///
/// # Safety
///
/// Same contract as [`__torajs_arr_print_any`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_print_any_at(arr: *const c_void, indent: u32) {
    unsafe { print_any_at(arr, indent) }
}

/// True when `v`'s NaN-box bit pattern is a *composite* value in
/// bun's `isPrimitive` sense (ConsoleObject.zig:1121-1135):
/// primitives are string / number / bool / null / undefined /
/// symbol / bigint; every other heap cell (Array, DynObj, struct
/// Obj, Map, Set, Promise, Date, RegExp, Closure, Weak*) is
/// composite. Cell detection mirrors `nan_box_is_cell_like` in
/// torajs-rc / torajs-value-drop (top 16 bits zero + low
/// TAG_BIT_TYPE_OTHER bit clear + non-zero).
#[inline]
unsafe fn slot_is_composite(v: u64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    if v == 0 || (v & TOP_16_MASK) != 0 || (v & TAG_BIT_TYPE_OTHER) != 0 {
        return false;
    }
    let header = unsafe { &*(v as *const torajs_rc::HeapHeader) };
    let tag = header.type_tag;
    tag != torajs_rc::Tag::Str as u16
        && tag != torajs_rc::Tag::Symbol as u16
        && tag != torajs_rc::Tag::BigInt as u16
        // RFC 20260722 chunk C — the generic undefined sentinel
        // cell IS the primitive `undefined` (bun single-lines
        // `[ undefined ]`); without this it read as composite and
        // forced the full-break open.
        && tag != torajs_rc::Tag::Undefined as u16
        // Primitive wrappers print inline (`"a"` / `[Number: 1]`)
        // and never force the full-break open (bun single-lines
        // `[ [Number: 1], 2 ]` — RFC 20260721 G5 tail).
        && tag != torajs_rc::Tag::NumberWrapper as u16
        && tag != torajs_rc::Tag::StringWrapper as u16
        && tag != torajs_rc::Tag::BooleanWrapper as u16
}

/// Indent-aware body of [`__torajs_arr_print_any`].
///
/// Full bun 1.3.14 break/wrap heuristic
/// (ConsoleObject.zig:2410-2591, inspect wrap trunk):
/// - open: full-break (`[\n<pad>` …) when `len > 10`, or the FIRST
///   element is composite (Array / object / Map / … — anything
///   non-primitive), or the running line estimate already exceeds
///   80; otherwise single-line (`[ `).
/// - mid-loop: elements join with `", "`; after each comma, if the
///   estimate exceeds 80 the line breaks and continues at the
///   element pad (this also upgrades a single-line opener to a
///   broken closer — the "first line inline" wrap shape).
/// - close: `\n<parent pad>]` when broken (no trailing comma),
///   otherwise ` ]`.
///
/// # Safety
/// Same contract as [`__torajs_arr_print_any`].
pub(crate) unsafe fn print_any_at(arr: *const c_void, indent: u32) {
    unsafe {
        let p = arr as *const u8;
        if p.is_null() {
            put_bytes(b"null");
            return;
        }
        // RFC 20260810 刀 D — the inspect walk reads raw slots; loud
        // reject (real sparse printing is a follow-up knife — it
        // needs bun's `empty x N` collapsed form).
        if crate::sparse_gate::sparse_tail_rejects(
            arr,
            b"sparse array tail is not yet supported in console.log\0".as_ptr(),
        ) {
            return;
        }
        // Any-dynamic-access RFC (20260704) S2 — a typed Array<T>
        // boxed into `any` reaches this walker through the Tag::Arr
        // print arms. Raw-scalar element kinds (recorded by
        // `__torajs_arr_mark_kind` at the boxing boundary) must NOT
        // be read as NaN-box AnyValues — dispatch to the typed
        // inline printers instead. ARR_KIND_HEAP slots are heap cell
        // pointers, which `__torajs_print_anyv_inline` already
        // renders correctly (cell-like passthrough), so HEAP and
        // UNSET both fall through to the AnyValue walk below.
        let header = &*(p as *const torajs_rc::HeapHeader);
        if header.flags & torajs_rc::FLAG_ARR_ANY == 0 {
            let kind = match header.arr_elem_kind() {
                torajs_rc::ARR_KIND_I64 => Some(crate::print_typed::TypedKind::I64),
                torajs_rc::ARR_KIND_F64 => Some(crate::print_typed::TypedKind::F64),
                torajs_rc::ARR_KIND_BOOL => Some(crate::print_typed::TypedKind::Bool),
                _ => None,
            };
            if let Some(kind) = kind {
                // Nested typed array — same break/wrap walker, at
                // this cell's indent (no estimate reset: the outer
                // composite's line is still live).
                crate::print_typed::print_typed_at(p, indent, kind);
                return;
            }
        }
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            put_bytes(b"[]");
            __torajs_inspect_line_add(2);
            return;
        }
        let head = *(p.add(ARR_HEAD_OFF) as *const u32);
        let slot_at =
            |i: u64| -> u64 { *(arr_data(p).add((head as usize + i as usize) * 8) as *const u64) };
        // bun's open-bracket decision (ConsoleObject.zig:2410-2462),
        // made once after peeking ONLY the first element's tag:
        // full-break when len > 10, or the first element is
        // composite, or the running line estimate already overflows.
        // Element indent = this array's indent + 2 (bun `indent += 1`
        // on entry, applied even on the single-line form).
        let my_indent = indent + 2;
        __torajs_inspect_line_add(2);
        let mut full =
            len > 10 || slot_is_composite(slot_at(0)) || __torajs_inspect_line_len() > 80;
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
                // Comma first, then the wrap check (bun's mid-loop
                // `printComma` → `goodTimeForANewLine`): the line
                // may run past 80 mid-element; only the position
                // *after* a comma triggers a break.
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
            __torajs_print_anyv_inline_at(slot_at(i), my_indent);
        }
        // Close-bracket decision is independent of the opener: a
        // single-line opener that wrapped mid-loop still closes on
        // its own line at the PARENT indent. It does NOT re-test the
        // 80-column estimate (probed 2026-07-05: bun keeps
        // `[ 7×"xxxxxxxxxx" ]` single-line at estimate 86). No
        // trailing comma for arrays.
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

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}
