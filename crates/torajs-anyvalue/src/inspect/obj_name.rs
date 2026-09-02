//! `__torajs_inspect_obj_name_prefix` — the name bun prints in front
//! of an ordinary object's `{` (rotation 562, closes 561-02 / 561-03).
//!
//! bun's `get_object_name` (`src/jsc/ConsoleObject.rs`) asks JSC for
//! the object's class name — `JSObject::calculatedClassName`
//! (`JSObject.cpp`): the `constructor` DATA property's function
//! name, own first and then the first one up [[Prototype]]; when
//! that is missing or spells `Object`, `Get(O, @@toStringTag)` if it
//! answers a String (the chain is walked). A name of `Object` or an
//! empty one prints nothing, and the walker then falls back to its
//! `[Object: null prototype]` prefix. So `K.prototype` prints
//! `K { … }` through its own `constructor`,
//! `{ [Symbol.toStringTag]: "T", a: 1 }` prints `T { a: 1 }`, and a
//! null-prototype object tagged `"Object"` keeps the null-prototype
//! prefix. A class instance never reaches here — its prefix is the
//! class name (`torajs-meta::struct_print`).

use core::ffi::c_void;

use torajs_rc::str_wtf8::StrWtf8;
use torajs_rc::{AnySlotTag, Tag};

use super::any::__torajs_print_str_cell_unquoted;
use super::formatters::{heap_type_tag, put_bytes};
use crate::member_get_own::dynobj_proto_pair;
use crate::method_call_object_proto_tag::to_string_tag_cell;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    /// torajs-dynobj — own-property probe pair (5 = absent).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// This crate's `name_get` — a closure's name Str, owned out.
    fn __torajs_closure_name_str(ptr: *mut c_void) -> *mut u8;
}

/// How many [[Prototype]] hops the `constructor` search takes at
/// most (bun's own walk stops at 5 user prototypes).
const MAX_HOPS: usize = 6;

/// JSC's default class name — bun prints nothing for it.
unsafe fn is_object_or_empty(cell: *const c_void) -> bool {
    let bytes = unsafe { StrWtf8::of(cell) };
    let bytes = bytes.as_bytes();
    bytes.is_empty() || bytes == b"Object"
}

/// A name Str cell and whether the caller owns it (a closure's name
/// comes out owned; a class object's own `name` entry is borrowed).
struct Name {
    cell: *const c_void,
    owned: bool,
}

/// The first `constructor` data property up the dynobj chain, as
/// its function's name (none when there is no such property, when
/// it is not a function, or when the chain leaves user objects). A
/// class object is a dynobj carrying its own `name` entry.
unsafe fn constructor_name(obj: *const c_void) -> Option<Name> {
    unsafe {
        let key = __torajs_str_alloc(b"constructor".as_ptr(), 11);
        let mut cur = obj;
        let mut name: Option<Name> = None;
        for _ in 0..MAX_HOPS {
            let tag = __torajs_dynobj_get_tag(cur, key as *const c_void);
            if tag == AnySlotTag::Heap as u64 {
                let v = __torajs_dynobj_get_value(cur, key as *const c_void) as *mut c_void;
                if !v.is_null() {
                    name = match heap_type_tag(v) {
                        t if t == Tag::Closure as u16 => Some(Name {
                            cell: __torajs_closure_name_str(v) as *const c_void,
                            owned: true,
                        }),
                        t if t == Tag::DynObj as u16 => class_object_name(v),
                        _ => None,
                    };
                }
                break;
            }
            if tag != AnySlotTag::Undef as u64 {
                break;
            }
            let (ptag, pp) = dynobj_proto_pair(cur);
            if ptag != AnySlotTag::Heap as u64
                || pp == 0
                || heap_type_tag(pp as *const c_void) != Tag::DynObj as u16
            {
                break;
            }
            cur = pp as *const c_void;
        }
        __torajs_str_drop(key as *mut c_void);
        name
    }
}

/// A class object's own `name` entry (a Str), borrowed.
unsafe fn class_object_name(class_obj: *const c_void) -> Option<Name> {
    unsafe {
        let key = __torajs_str_alloc(b"name".as_ptr(), 4);
        let tag = __torajs_dynobj_get_tag(class_obj, key as *const c_void);
        let cell = if tag == AnySlotTag::Heap as u64 {
            __torajs_dynobj_get_value(class_obj, key as *const c_void) as *const c_void
        } else {
            core::ptr::null()
        };
        __torajs_str_drop(key as *mut c_void);
        (!cell.is_null() && heap_type_tag(cell) == Tag::Str as u16)
            .then_some(Name { cell, owned: false })
    }
}

/// Print `<name> ` for `obj` and answer 1, or print nothing and
/// answer 0 (the walker then decides on the null-prototype prefix).
///
/// # Safety
/// `obj` is a live dynobj cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_inspect_obj_name_prefix(obj: *const c_void) -> i32 {
    unsafe {
        if let Some(name) = constructor_name(obj) {
            let named = !is_object_or_empty(name.cell);
            if named {
                __torajs_print_str_cell_unquoted(name.cell);
                put_bytes(b" ");
            }
            if name.owned {
                __torajs_str_drop(name.cell as *mut c_void);
            }
            if named {
                return 1;
            }
        }
        if let Some(tag) = to_string_tag_cell(obj as u64) {
            let (_, payload) = tag.pair();
            let cell = payload as *const c_void;
            if !is_object_or_empty(cell) {
                __torajs_print_str_cell_unquoted(cell);
                put_bytes(b" ");
                return 1;
            }
        }
        0
    }
}
