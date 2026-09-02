//! 562-06 — `console.log(<class>)` prints `[class Z]`, not the
//! property bag the class object is.
//!
//! A class object is a dynobj carrying `name` / `length` /
//! `prototype` (§10.2.3 MakeConstructor), so tr's ordinary-object
//! walker printed exactly that:
//! `{ name: "Z", prototype: Z { m: [Function: m] }, length: 0 }`.
//! bun asks JSC whether the value is a class constructor and prints
//! `[class <name>]` — `[class D extends B]` when it extends another
//! class, `[class (anonymous)]` when it has no name — with no block
//! at all, at every depth (`[ [class Z], [Function: f1] ]`).
//!
//! "Is this cell a class object" is answered by the registry itself:
//! `CLASSES_BY_TAG_IMM` holds every registered class object, so a
//! pointer match over it both decides the question and names the
//! tag. The superclass is the class object's own [[Prototype]] —
//! a second match over the same table, which is exactly the
//! "extends a USER class" test bun's spelling wants (extending
//! nothing lands on %Function.prototype%, which is in no slot).
//!
//! 567-01 — the name this form prints is the DEFINITION name, not
//! the own `name` property. They are two different questions and
//! they come apart: `Object.defineProperty(C, "name", …)` moves
//! `C.name`, and bun keeps printing `[class C]`, because JSC's
//! inspect asks the class about its source, not about its property
//! bag. The same split showed up in 565-03 (a computed member's
//! name is a runtime fact its source cannot spell) and 566-01/02
//! (an accessor's two faces). The definition name already has a
//! table — `__torajs_class_name_table`, keyed by the same class tag
//! this file already resolves, and already the prefix an INSTANCE
//! prints — so this form reads it instead of the bag, on both the
//! class's own name and the `extends` parent's.

use core::ffi::c_void;

use crate::classmeta::{__torajs_class_cell_raw, MAX_CLASSES};
use crate::reflect::{ANY_HEAP, PROTO_SLOT_KEY, TAG_STR, alloc_str_key, heap_type_tag};

unsafe extern "C" {
    /// torajs-dynobj — own-entry probe pair (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const u8) -> u64;
    /// torajs-anyvalue::inspect — a Str cell's bytes, no quotes.
    fn __torajs_print_str_cell_unquoted(cell: *const c_void);
    /// torajs-anyvalue::inspect — the interned `.name` of a builtin
    /// ctor cell that reads as a class, NULL otherwise (564-03).
    fn __torajs_builtin_ctor_class_name(cell: *const c_void) -> *const c_void;
    /// torajs-structmeta — the class-tag → source-text name table
    /// (W-J Phase A3c), the same rodata an instance's printed prefix
    /// reads. Its rows carry `class_display_name`, so an anonymous
    /// class expression's row is empty, never the `__ClassExpr_<id>`
    /// synth.
    fn __torajs_struct_class_name(class_tag: u32) -> crate::struct_print::StrSlice;
    /// torajs-io — per-byte stdout writer.
    fn __torajs_io_putc_out(c: i32) -> i32;
    /// torajs-anyvalue::inspect — bun `estimated_line_length` mirror.
    fn __torajs_inspect_line_add(n: u32);
    fn __torajs_str_drop(s: *mut u8);
}

unsafe fn put_bytes(s: &[u8]) {
    for &b in s {
        unsafe { __torajs_io_putc_out(b as i32) };
    }
    unsafe { __torajs_inspect_line_add(s.len() as u32) };
}

/// The registry tag whose class object IS `cell`, or `None`.
///
/// # Safety
/// `cell` is a live heap cell.
unsafe fn tag_of_class_object(cell: *const c_void) -> Option<i64> {
    if cell.is_null() {
        return None;
    }
    for tag in 0..MAX_CLASSES as i64 {
        let v = __torajs_class_cell_raw(tag);
        if v != 0 && v as *const c_void == cell {
            return Some(tag);
        }
    }
    None
}

/// The definition name recorded for `tag`, or `None` when the row
/// is absent or empty — an anonymous class expression in no naming
/// position, and a class the layout table never got a row for.
fn def_name(tag: i64) -> Option<&'static [u8]> {
    let cn = unsafe { __torajs_struct_class_name(tag as u32) };
    if cn.ptr.is_null() || cn.len == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(cn.ptr, cn.len) })
}

/// The class object's own `name` Str cell (borrowed), or null.
///
/// # Safety
/// `cell` is a live dynobj.
unsafe fn own_str_entry(cell: *const c_void, key_bytes: &[u8]) -> *const c_void {
    unsafe {
        let key = alloc_str_key(key_bytes);
        let tag = __torajs_dynobj_get_tag(cell, key);
        let v = if tag == ANY_HEAP as u64 {
            __torajs_dynobj_get_value(cell, key) as *const c_void
        } else {
            core::ptr::null()
        };
        __torajs_str_drop(key);
        v
    }
}

/// Print `[class <name>]` for a class object and answer 1; answer 0
/// for anything else, leaving the caller's ordinary-object form.
///
/// # Safety
/// `cell` is NULL or a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_object_print(cell: *const c_void) -> i32 {
    let Some(tag) = (unsafe { tag_of_class_object(cell) }) else {
        return 0;
    };
    unsafe {
        put_bytes(b"[class ");
        // 563-05 — an anonymous class expression's ES name is the
        // empty string (§8.4 NamedEvaluation found no binding), and
        // bun prints `[class (anonymous)]` for it, not `[class ]`.
        match def_name(tag) {
            Some(n) => put_bytes(n),
            None => put_bytes(b"(anonymous)"),
        }
        // `extends` names the SUPERCLASS, which is this class
        // object's [[Prototype]]. That is a registered class object
        // when the class extends a user class, and an interned
        // builtin ctor cell when it extends a builtin (564-03) —
        // the latter names itself only when it reads as a class, so
        // `class P extends Promise {}` prints `[class P]`.
        let parent = own_str_entry(cell, PROTO_SLOT_KEY);
        let ptag = if parent.is_null() {
            None
        } else {
            tag_of_class_object(parent)
        };
        match ptag {
            Some(pt) => {
                if let Some(n) = def_name(pt) {
                    put_bytes(b" extends ");
                    put_bytes(n);
                }
            }
            None => {
                let pname = if parent.is_null() {
                    core::ptr::null()
                } else {
                    __torajs_builtin_ctor_class_name(parent)
                };
                if !pname.is_null() && heap_type_tag(pname) == TAG_STR {
                    put_bytes(b" extends ");
                    __torajs_print_str_cell_unquoted(pname);
                }
            }
        }
        put_bytes(b"]");
    }
    1
}
