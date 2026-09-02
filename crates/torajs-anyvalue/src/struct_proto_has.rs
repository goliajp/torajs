//! Class-prototype membership probe for a `Tag::Obj` receiver's `in`
//! chain face (`__torajs_in_op_any_str` TAG_OBJ link, rotation 149).
//!
//! A class method / class accessor is the CLASS PROTOTYPE's property,
//! not the instance's own (§ B.3.5 class semantics via MakeMethod on
//! the prototype): `c.hasOwnProperty("m")` answers false while
//! `"m" in c` answers true. The own face (`prop_has`) therefore must
//! NOT list these — this probe is the separate chain link between the
//! instance's own face and the `Object.prototype` root.
//!
//! Two tables back it, both keyed off the layout the cell's
//! class_tag resolves: `__torajs_struct_method_find` (plain methods,
//! the dispatch table `struct_method` calls through) and
//! `__torajs_struct_accessor_method_find` (class accessor adapters;
//! EITHER half makes the property present, matching
//! `struct_probe::resolve`'s read/write split). Object-literal
//! accessor SLOTS stay out — those are own properties the own face
//! already answers.

use core::ffi::c_void;

use crate::prop_has::key_bytes;
use crate::struct_probe::{KIND_GETTER, KIND_SETTER};

unsafe extern "C" {
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
    ) -> *const c_void;
    fn __torajs_struct_accessor_method_find(
        layout: *const c_void,
        name: *const u8,
        name_len: u32,
        kind: u8,
    ) -> *const c_void;
}

/// Offset of the u32 class_tag inside a `Tag::Obj` heap block
/// (mirrors `struct_probe::OBJ_CLASS_TAG_OFF`).
const OBJ_CLASS_TAG_OFF: usize = 8;

/// 1 when `key` names a class-prototype member (method or either
/// accessor half) of the struct cell `ptr`; 0 otherwise — including
/// object literals, whose layouts register no dispatch table.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_proto_member_has(
    ptr: *const c_void,
    key: *const c_void,
) -> i64 {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return 0;
    }
    let k = unsafe { key_bytes(key) };
    let (name, len) = (k.as_ptr(), k.len());
    if !unsafe { __torajs_struct_method_find(layout, name, len) }.is_null() {
        return 1;
    }
    let acc_half = |kind: u8| {
        !unsafe { __torajs_struct_accessor_method_find(layout, name, len, kind) }.is_null()
    };
    (acc_half(KIND_GETTER) || acc_half(KIND_SETTER)) as i64
}
