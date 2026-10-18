//! The object cast for a plan whose class nodes list their class's
//! prototype-chain keys (`PROTO_KEYS`): an object that is no instance
//! of the class is copied into a fresh one, whose methods and accessors
//! are the class's, so an object that owns a property under one of
//! those names would answer the class's member instead of its own. Such
//! an object does not cross. The lowering calls these entries only for
//! such a plan, so a program that casts into no class with a method or
//! accessor links none of this code.

use core::ffi::c_void;

use super::members::member;
use super::obj::{__torajs_str_alloc, __torajs_str_drop, run};
use super::plan::Plan;
use crate::nanbox::AnyValue;

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

/// Whether `v` owns (§7.3.12 HasOwnProperty) a property under one of
/// the keys the OBJ node at `at` lists after its fields.
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
        let key = unsafe { __torajs_str_alloc(plan.base.add(pos + 2), len as i64) };
        let has = unsafe { crate::prop_has::__torajs_any_own_prop_has(v, key.cast()) };
        unsafe { __torajs_str_drop(key.cast()) };
        if has != 0 {
            return true;
        }
        pos += 2 + len;
    }
    false
}
