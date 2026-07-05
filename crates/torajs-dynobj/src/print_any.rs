//! `console.log(obj)` for arbitrary dynobj (plain object literals,
//! `Object.entries` rows, etc) — nested-print substrate trunk
//! Commit 3.
//!
//! Walks a dynobj's dense entry array via
//! `__torajs_dynobj_iter_{len,key,value,flags}` in insertion order
//! (CPython-3.7 compact-dict shape — `torajs-dynobj` since session
//! 85 `88c9778`), filters out tombstones (`key == null`) and
//! non-enumerable entries (`BUCKET_FLAG_ENUMERABLE` bit clear), and
//! emits each `<key>: <value>` pair through
//! `__torajs_print_anyv_inline` (Commit 1 substrate).
//!
//! Output shape (bun-parity for plain object literals):
//! - empty (no enumerable own props) → `{}` (no '\n')
//! - non-empty → `{\n  k: v,\n  k2: v2,\n}` (no trailing '\n', but
//!   keeps the final `}` on its own line per bun's pretty form;
//!   trailing comma on the last entry mirrors bun exactly)
//!
//! Key emission: keys are borrowed Str cell pointers (returned by
//! `__torajs_dynobj_iter_key`). They are reinterpreted as a NaN-box
//! AnyValue (raw cell ptr is a valid NaN-box cell encoding —
//! `nanbox::box_void_ptr`) and dispatched through
//! `__torajs_print_anyv_inline` which routes them via its Tag::Str
//! / Tag::Substr arm. Result: keys print **unquoted** (`a` not
//! `"a"`) matching bun's obj-literal form (Map / Set quote their
//! keys differently — that's a later commit's concern).
//!
//! ## Newline policy
//!
//! Commit 3 emits no trailing '\n' after the closing `}`. The
//! Commit 4 `__torajs_print_anyv` Tag::Obj arm + SSA dispatcher
//! obj-elem case append '\n' when the outer console.log call
//! terminates.

use core::ffi::c_void;

use crate::iter::{
    __torajs_dynobj_iter_flags, __torajs_dynobj_iter_key, __torajs_dynobj_iter_len,
    __torajs_dynobj_iter_value,
};
use crate::layout::BUCKET_FLAG_ENUMERABLE;

unsafe extern "C" {
    /// Indent-threaded inline AnyValue printer from
    /// `torajs-anyvalue::inspect` (inspect indent trunk).
    fn __torajs_print_anyv_inline_at(v: u64, indent: u32);
    /// Per-byte stdout writer (`libtorajs_io.a`). Same buffer
    /// shared with IR-emitted `print_*` and Str / Arr printers.
    fn __torajs_io_putc_stdout(c: i32) -> i32;
    /// Emit a Str / Substr cell's bytes WITHOUT the `"..."` quote
    /// wrapper — for dynobj keys which bun renders unquoted
    /// (`{ a: 1 }` not `{ "a": 1 }`). Commit 4 substrate helper
    /// in `torajs-anyvalue::inspect`.
    fn __torajs_print_str_cell_unquoted(cell: *const c_void);
}

#[inline]
unsafe fn put_byte(b: u8) {
    unsafe { __torajs_io_putc_stdout(b as i32) };
}

#[inline]
unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { put_byte(b) };
    }
}

/// `console.log(obj)` recursive walker for dynobj. Emits the bun
/// multi-line `{\n  k: v,\n}` pretty form with no trailing newline.
///
/// # Safety
///
/// `obj` must be either NULL or a valid dynobj heap object whose
/// header / entry array / index follow the layout asserted by
/// `torajs-dynobj::layout`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_print_any(obj: *const c_void) {
    unsafe { obj_print_any_at(obj, 0) }
}

/// Indent-threaded export of the walker (inspect indent trunk) —
/// `indent` is this object's own indent column: fields pad at
/// `indent + 2`, the closing `}` at `indent` (bun's uniform depth*2
/// model). Values thread `indent + 2` as their own indent so nested
/// composites keep descending.
///
/// # Safety
///
/// Same contract as [`__torajs_obj_print_any`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_print_any_at(obj: *const c_void, indent: u32) {
    unsafe { obj_print_any_at(obj, indent) }
}

unsafe fn obj_print_any_at(obj: *const c_void, indent: u32) {
    unsafe {
        if obj.is_null() {
            put_bytes(b"null");
            return;
        }
        let len = __torajs_dynobj_iter_len(obj);
        let mut any_emitted = false;
        for i in 0..len {
            let key = __torajs_dynobj_iter_key(obj, i);
            if key.is_null() {
                // Tombstone — entry was deleted; skip per iter
                // contract (key==null sentinel for vacated slot).
                continue;
            }
            let flags = __torajs_dynobj_iter_flags(obj, i);
            if flags & BUCKET_FLAG_ENUMERABLE == 0 {
                // Hidden property (W/E/C all-false or just
                // enumerable=false) — skip per
                // for-in / console.log own enumerable contract.
                continue;
            }
            if !any_emitted {
                put_bytes(b"{\n");
                any_emitted = true;
            } else {
                put_bytes(b",\n");
            }
            put_indent(indent + 2);
            // Key — borrowed Str cell ptr. Dynobj keys are always
            // Tag::Str (dynobj symbol-keyed entries are not yet
            // supported per W-N-c L3b). bun renders obj keys
            // unquoted (`{ a: 1 }`) so we use the unquoted helper
            // instead of __torajs_print_anyv_inline_at (which adds
            // `"..."` for nested-context strings).
            __torajs_print_str_cell_unquoted(key);
            put_bytes(b": ");
            // Value — already a NaN-box AnyValue per iter_value's
            // u64 return contract. Its own indent is this field
            // row's column (indent + 2).
            let v = __torajs_dynobj_iter_value(obj, i);
            __torajs_print_anyv_inline_at(v, indent + 2);
        }
        if !any_emitted {
            put_bytes(b"{}");
        } else {
            put_bytes(b",\n");
            put_indent(indent);
            put_byte(b'}');
        }
    }
}

#[inline]
unsafe fn put_indent(n: u32) {
    for _ in 0..n {
        unsafe { put_byte(b' ') };
    }
}
