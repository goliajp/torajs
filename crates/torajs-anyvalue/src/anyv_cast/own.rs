//! The object cast for a plan whose class nodes list their class's
//! prototype-chain keys (`PROTO_KEYS`): an object that is no instance
//! of the class is copied into a fresh one, whose methods and accessors
//! are the class's, so an object that holds a property under one of
//! those names would answer the class's member instead of its own. Such
//! an object does not cross. The lowering calls these entries only for
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

use core::ffi::c_void;

use torajs_rc::Tag;
use torajs_rc::builtin_proto::{__torajs_builtin_proto_tag_of, OBJECT_PROTO_TAG};

use super::members::member;
use super::obj::{__torajs_str_alloc, __torajs_str_drop, run};
use super::plan::Plan;
use crate::member_get::recv_cell;
use crate::nanbox::AnyValue;
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::prop_has::__torajs_any_own_prop_has;

unsafe extern "C" {
    /// torajs-str — the well-known symbol a `__sym_Symbol_<x>__`
    /// member name stands for, owned +1; NULL when it names none.
    fn __torajs_symbol_well_known_of_member(name: *const u8, len: usize) -> *mut c_void;
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
}

/// [`super::obj::__torajs_anyv_cast_obj`] for a plan with a class node
/// listing keys.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_own(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { run(Plan::<true>::new(plan, vtables), v, site) }
}

/// The same, for a plan that also has an optional member among its
/// fields (`super::members`).
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_members_own(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    let mut plan = Plan::<true>::new(plan, vtables);
    plan.member = Some(member);
    unsafe { run(plan, v, site) }
}

/// Whether `v` holds (module doc) a property under one of the keys the
/// OBJ node at `at` lists after its fields.
///
/// # Safety
/// `at` is an OBJ node with the `PROTO_KEYS` flag; `v` is a live
/// object cell.
pub(super) unsafe fn owns_key<const OWN: bool>(plan: &Plan<OWN>, v: AnyValue, at: usize) -> bool {
    let mut pos = at + 16;
    for _ in 0..plan.u16(at + 14) {
        pos = plan.u32(pos) as usize;
    }
    let count = plan.u16(pos);
    pos += 2;
    for _ in 0..count {
        let len = plan.u16(pos) as usize;
        let name = unsafe { plan.base.add(pos + 2) };
        let sym = unsafe { __torajs_symbol_well_known_of_member(name, len) };
        let has = if sym.is_null() {
            let key = unsafe { __torajs_str_alloc(name, len as i64) };
            let has = unsafe { holds(v, key.cast()) };
            unsafe { __torajs_str_drop(key.cast()) };
            has
        } else {
            let has = unsafe { holds(v, sym) };
            unsafe { __torajs_rc_dec(sym) };
            has
        };
        if has {
            return true;
        }
        pos += 2 + len;
    }
    false
}

/// Whether `v`, or a prototype it inherits from ahead of
/// %Object.prototype%, owns `key` (module doc).
///
/// # Safety
/// `v` is a live object cell; `key` a live Str or Symbol cell.
unsafe fn holds(mut v: AnyValue, key: *const c_void) -> bool {
    loop {
        if unsafe { __torajs_any_own_prop_has(v, key) } != 0 {
            return true;
        }
        let Some((ptr, tag)) = recv_cell(v) else {
            return false;
        };
        if tag != Tag::DynObj as u16 {
            return false;
        }
        let Some(parent) = (unsafe { crate::member_get_own::user_proto_cell(ptr) }) else {
            return false;
        };
        let root = unsafe { __torajs_builtin_proto_tag_of(parent as *const c_void) };
        if root == OBJECT_PROTO_TAG as i64 {
            return false;
        }
        v = unsafe { __torajs_anyv_box_from_pair(4, parent as i64) };
    }
}
