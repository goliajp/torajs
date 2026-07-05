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

use crate::layout::{ARR_LEN_OFF, ARR_SLOTS_OFF};
use crate::print::{put_byte, put_bytes};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// Indent-threaded inline AnyValue printer from
    /// `torajs-anyvalue::inspect` (inspect indent trunk).
    /// Cross-staticlib extern — resolved at `tr build` link time.
    /// Takes the raw 8 bytes of an `AnyValue` slot as `u64` plus the
    /// composite's own indent column and writes its bun-form
    /// rendering through `__torajs_io_putc_stdout` with no trailing
    /// newline.
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
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

/// True when `v`'s bit pattern is a heap cell pointer whose header
/// tag is `Tag::Arr`. Mirrors `nan_box_is_cell_like` in torajs-rc /
/// torajs-value-drop (top 16 bits zero + low TAG_BIT_TYPE_OTHER bit
/// clear + non-zero).
#[inline]
unsafe fn slot_is_arr_cell(v: u64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    if v == 0 || (v & TOP_16_MASK) != 0 || (v & TAG_BIT_TYPE_OTHER) != 0 {
        return false;
    }
    let header = unsafe { &*(v as *const torajs_rc::HeapHeader) };
    header.type_tag == torajs_rc::Tag::Arr as u16
}

/// Indent-aware body of [`__torajs_arr_print_any`].
///
/// Break rule (bun parity, probed 2026-07-04): an array whose
/// elements are ALL arrays renders multi-line —
/// `[\n<indent+2><e0>, <e1>\n<indent>]`, siblings joined by `", "`
/// on the same line, two spaces of indent per nesting level. Any
/// scalar element keeps the whole level single-line. (bun's wider
/// inspect heuristics — leading-composite break, width-driven wrap,
/// object elements — are a separate L3b trunk; those shapes keep
/// tr's current single-line form.)
///
/// Array-cell slots recurse HERE (carrying indent) rather than
/// through `__torajs_print_anyv_inline`'s Tag::Arr branch, which
/// would reset indent to 0.
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
            match header.arr_elem_kind() {
                torajs_rc::ARR_KIND_I64 => {
                    crate::print_inline::__torajs_arr_print_i64_inline(arr);
                    return;
                }
                torajs_rc::ARR_KIND_F64 => {
                    crate::print_inline::__torajs_arr_print_f64_inline(arr);
                    return;
                }
                torajs_rc::ARR_KIND_BOOL => {
                    crate::print_inline::__torajs_arr_print_bool_inline(arr);
                    return;
                }
                _ => {}
            }
        }
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if len == 0 {
            put_bytes(b"[]");
            return;
        }
        let head = *(p.add(ARR_HEAD_OFF) as *const u32);
        let slot_at = |i: u64| -> u64 {
            *(p.add(ARR_SLOTS_OFF + (head as usize + i as usize) * 8) as *const u64)
        };
        let all_arr = (0..len).all(|i| slot_is_arr_cell(slot_at(i)));
        if all_arr {
            put_bytes(b"[\n");
            put_indent(indent + 2);
            for i in 0..len {
                if i > 0 {
                    put_bytes(b", ");
                }
                print_any_at(slot_at(i) as *const c_void, indent + 2);
            }
            put_byte(b'\n');
            put_indent(indent);
            put_byte(b']');
            return;
        }
        put_bytes(b"[ ");
        for i in 0..len {
            if i > 0 {
                put_bytes(b", ");
            }
            // Element indent = this array's indent + 2 — bun pads
            // nested composites by depth even when this level stays
            // single-line (`[ 1, {\n    k: 1,\n  } ]` puts the
            // object's fields at column 4, its closer at 2).
            __torajs_print_anyv_inline_at(slot_at(i), indent + 2);
        }
        // Trailing " ]" — bun emits a space before the closing
        // bracket only when the array is non-empty (the empty arm
        // above already short-circuited with `[]`).
        put_byte(b' ');
        put_byte(b']');
    }
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}
