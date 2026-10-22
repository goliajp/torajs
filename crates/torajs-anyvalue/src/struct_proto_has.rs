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
    fn __torajs_struct_method_count(layout: *const c_void) -> u32;
    /// torajs-structmeta — see `prop_has`: 255 for a plain name, a
    /// kind byte for a synthetic accessor slot spelling.
    fn __torajs_accessor_name_kind(name: *const u8, name_len: u32) -> u8;
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
    // A re-parented instance's class is no longer on its chain.
    if unsafe { crate::member_get_layout::obj_user_proto_raised(ptr) } {
        return 0;
    }
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    i64::from(unsafe { class_member_has(class_tag, key) })
}

/// Whether the class `class_tag` names registers `key` as a prototype
/// member (method or either accessor half).
unsafe fn class_member_has(class_tag: u32, key: *const c_void) -> bool {
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    // an object literal's layout registers no member table at all
    if layout.is_null() || unsafe { __torajs_struct_method_count(layout) } == 0 {
        return false;
    }
    let k = unsafe { key_bytes(key) };
    let (name, len) = (k.as_ptr(), k.len());
    // A class accessor's adapters are registered under their
    // synthetic slot names too, so the probes below matched
    // `"__getter_d" in c` — the same leak the own face had, one
    // chain link over. Both now ask the one authority.
    if unsafe { __torajs_accessor_name_kind(name, len) } != 255 {
        return false;
    }
    if !unsafe { __torajs_struct_method_find(layout, name, len) }.is_null() {
        return true;
    }
    let acc_half = |kind: u8| {
        !unsafe { __torajs_struct_accessor_method_find(layout, name, len, kind) }.is_null()
    };
    acc_half(KIND_GETTER) || acc_half(KIND_SETTER)
}

unsafe extern "C" {
    /// torajs-rc — §13.10.1 `in` over a boxed receiver, the whole walk.
    fn __torajs_in_op_any_str(v: i64, key: *const u8) -> bool;
    /// torajs-dispatch — the link-judged seam over
    /// [`__torajs_struct_lacks_in_user_proto_impl`].
    fn __torajs_struct_lacks_in_user_proto_slow(props: *const c_void, key: *const c_void) -> i64;
}

/// §7.3.12 HasProperty of `key` on a struct cell that lacks the declared
/// member `key` (a literal left it out, or `delete` took it): the walk
/// resumes past the own face. The compiler hands over the cell's header
/// flags, its `+8` word and its expando dict, never the cell, so a cell
/// that lives in a frame stays there. A re-parented cell walks its user
/// [[Prototype]] (a seam the link judgment stubs while nothing can
/// re-parent a struct); any other the class prototype, then
/// `Object.prototype`.
///
/// # Safety
/// `props` is the cell's expando dict (NULL when it has none); `key` is
/// a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_lacks_in(
    flags: i64,
    tag_word: i64,
    props: *const c_void,
    key: *const c_void,
) -> i64 {
    if flags as u16 & torajs_rc::FLAG_OBJ_USER_PROTO != 0 {
        return unsafe { __torajs_struct_lacks_in_user_proto_slow(props, key) };
    }
    if unsafe { class_member_has(tag_word as u32, key) } {
        return 1;
    }
    i64::from(unsafe { crate::method_support_proto_chain::object_root_expando_owns(key) })
}

/// HasProperty of `key` along the user [[Prototype]] a re-parented
/// struct cell keeps in its expando dict `props`.
///
/// # Safety
/// `props` is the dict of a cell with `FLAG_OBJ_USER_PROTO` raised;
/// `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_struct_lacks_in_user_proto_impl(
    props: *const c_void,
    key: *const c_void,
) -> i64 {
    let parent = unsafe { crate::member_get_own::__torajs_dynobj_user_proto(props) };
    i64::from(!parent.is_null() && unsafe { __torajs_in_op_any_str(parent as i64, key.cast()) })
}
