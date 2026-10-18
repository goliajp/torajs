//! The object cast for a plan whose class nodes list their class's
//! prototype-chain keys: an object that is no instance of the class is
//! copied into a fresh one, whose methods and accessors are the
//! class's, so an object that holds a property under one of those
//! names would answer the class's member instead of its own. Such an
//! object does not cross. The lowering calls these entries only for
//! such a plan, so a program that casts into no class with a method or
//! accessor links none of this code.
//!
//! An object holds a key when it owns it, or when a prototype it
//! inherits from ahead of %Object.prototype% does: the copy inherits
//! from %Object.prototype% too, so what the object answers from there
//! the copy answers alike. A prototype link is followed through
//! ordinary objects; a prototype of another kind (a class instance, a
//! builtin's prototype other than %Object.prototype%) is probed for its
//! own properties only. A key `__sym_Symbol_<x>__` (the class member
//! `[Symbol.<x>]`) is probed as the well-known symbol it names.
//!
//! The walk is torajs-anyvalue's, instantiated here for [`Own`]: the
//! crate is an archive member of its own, so the walk's code is parsed
//! and linked only where one of the two entries is called.

#![no_std]

use core::ffi::c_void;

use torajs_anyvalue::cast_own_support::{
    __torajs_any_own_prop_has, __torajs_anyv_box_from_pair, Walk, cast_obj, cast_obj_members,
};
use torajs_anyvalue::nanbox::{AnyValue, as_void_ptr, is_cell};
use torajs_rc::Tag;
use torajs_rc::builtin_proto::{__torajs_builtin_proto_tag_of, OBJECT_PROTO_TAG};

unsafe extern "C" {
    /// torajs-str — the well-known symbol a `__sym_Symbol_<x>__`
    /// member name stands for, owned +1; NULL when it names none.
    fn __torajs_symbol_well_known_of_member(name: *const u8, len: usize) -> *mut c_void;
    fn __torajs_str_alloc(p: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    /// torajs-anyvalue — the dynobj's user [[Prototype]] (borrowed),
    /// NULL when it carries none (null-proto shape / implicit chain).
    fn __torajs_dynobj_user_proto(ptr: *const c_void) -> *mut c_void;
}

/// The walk that refuses an object holding one of a class node's keys.
pub enum Own {}

impl Walk for Own {
    const OWN: bool = true;

    unsafe fn holds(v: AnyValue, name: *const u8, len: usize) -> bool {
        let sym = unsafe { __torajs_symbol_well_known_of_member(name, len) };
        if sym.is_null() {
            let key = unsafe { __torajs_str_alloc(name, len as i64) };
            let has = unsafe { holds(v, key.cast()) };
            unsafe { __torajs_str_drop(key.cast()) };
            has
        } else {
            let has = unsafe { holds(v, sym) };
            unsafe { __torajs_rc_dec(sym) };
            has
        }
    }
}

/// `__torajs_anyv_cast_obj` (torajs-anyvalue) for a plan with a class
/// node listing keys.
///
/// # Safety
/// As `__torajs_anyv_cast_obj`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_own(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { cast_obj::<Own>(v, plan, vtables, site) }
}

/// The same, for a plan that also has an optional member among its
/// fields (`__torajs_anyv_cast_obj_members`).
///
/// # Safety
/// As `__torajs_anyv_cast_obj`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_members_own(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { cast_obj_members::<Own>(v, plan, vtables, site) }
}

/// Whether `v`, or a prototype it inherits from ahead of
/// %Object.prototype%, owns `key` (crate doc).
///
/// # Safety
/// `v` is a live object cell; `key` a live Str or Symbol cell.
unsafe fn holds(mut v: AnyValue, key: *const c_void) -> bool {
    loop {
        if unsafe { __torajs_any_own_prop_has(v, key) } != 0 {
            return true;
        }
        if !is_cell(v) {
            return false;
        }
        let ptr = as_void_ptr(v);
        if unsafe { ptr.cast::<u8>().add(4).cast::<u16>().read() } != Tag::DynObj as u16 {
            return false;
        }
        let parent = unsafe { __torajs_dynobj_user_proto(ptr) };
        if parent.is_null() {
            return false;
        }
        let root = unsafe { __torajs_builtin_proto_tag_of(parent) };
        if root == OBJECT_PROTO_TAG as i64 {
            return false;
        }
        v = unsafe { __torajs_anyv_box_from_pair(4, parent as i64) };
    }
}
