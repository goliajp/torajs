//! The object cast into the member of an object literal whose declared
//! type gives it an object layout. Such a slot is read and released
//! through more than one static view (`type S = { k: W }` read as
//! `{ k: N }`), and its release frees a cell by the cell's own size, so
//! a struct or class cell of any layout may sit in it. Only a value
//! that is no `Tag::Obj` cell is checked:
//!
//! - an instance of a class crosses with +1 and keeps its identity;
//! - a struct cell is cast by the plan (a cell of the layout as it is,
//!   one with the layout's fields copied), and crosses with +1 as it is
//!   where the plan does not take it;
//! - anything else is cast as [`super::obj`] casts it.
//!
//! Kept apart from `super::obj`, so a program with no such member links
//! none of it.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::cell_tag;
use super::obj::{class_instance, run};
use super::plan::{Fail, Plan};
use crate::nanbox::{AnyValue, as_void_ptr, is_cell};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
}

/// [`super::obj::__torajs_anyv_cast_obj`] into an object literal's
/// member: a `Tag::Obj` cell of any layout crosses.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_keep(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { keep(Plan::new(plan, vtables), v, site) }
}

/// [`__torajs_anyv_cast_obj_keep`] for a plan with an optional member
/// among the fields of its object nodes.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_members_keep(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    let mut plan = Plan::new(plan, vtables);
    plan.member = Some(super::members::member);
    unsafe { keep(plan, v, site) }
}

/// The cast `plan` makes of `v` into an object literal's member.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[inline(always)]
unsafe fn keep(plan: Plan, v: AnyValue, site: *const u8) -> *mut c_void {
    if !is_cell(v) || unsafe { cell_tag(v) } != Tag::Obj as u16 {
        return unsafe { run(plan, v, site) };
    }
    let p = as_void_ptr(v);
    if !unsafe { class_instance(p) } {
        match unsafe { plan.cast(4, v) } {
            Ok(bits) => return bits as *mut c_void,
            Err(Fail::Thrown) => return core::ptr::null_mut(),
            Err(Fail::Mismatch) => {}
        }
    }
    unsafe { __torajs_rc_inc(p) };
    p
}
