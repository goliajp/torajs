//! The object cast for a plan with an optional member among its fields
//! (`MEMBER` field nodes): the lowering calls this entry only then, so the
//! code that tells a lacking member from one holding `undefined`, and
//! parks a `null` an optional `number` member's f64 slot cannot spell,
//! is linked only into a program that casts such a member.

use core::ffi::c_void;

use super::obj::{__torajs_str_alloc, __torajs_str_drop, __torajs_throw_check, run};
use super::plan::{Fail, Field, NUM, OPT, Plan, Walk};
use crate::arg_struct_coerce::OBJ_HEADER_SIZE;
use crate::nanbox::{AnyValue, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-dynobj — the deletion record of a declared field, for a
    /// slot past the ones the absent set can name.
    fn __torajs_obj_declared_field_delete(obj: *mut c_void, key: *mut c_void) -> i64;
}

/// The slots the absent set names: bit `i` of the u32 at
/// [`ABSENT_SET_OFF`] for slot `i` (torajs-meta `struct_absent`).
const ABSENT_SET_SLOTS: usize = 32;
const ABSENT_SET_OFF: usize = 12;
/// The header's flags half-word.
const OBJ_FLAGS_OFF: usize = 6;

/// [`super::obj::__torajs_anyv_cast_obj`] for a plan with an optional
/// member among the fields of its object nodes.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_cast_obj_members(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    unsafe { cast_obj_members::<super::Plain>(v, plan, vtables, site) }
}

/// [`__torajs_anyv_cast_obj_members`] walked by `W`.
///
/// # Safety
/// As [`super::obj::__torajs_anyv_cast_obj`].
#[inline(always)]
pub unsafe fn cast_obj_members<W: Walk>(
    v: AnyValue,
    plan: *const u8,
    vtables: *const *mut c_void,
    site: *const u8,
) -> *mut c_void {
    let mut plan = Plan::<W>::new(plan, vtables);
    plan.member = Some(member);
    unsafe { run(plan, v, site) }
}

/// The optional member `at` of the copy, read from `v` as `g` (owned):
/// a member `v` lacks is recorded absent on the cell, and a `null` for
/// an optional `number` member goes to the field's sidecar.
///
/// # Safety
/// `at.cell` is the fresh cell being built, with its header written;
/// `v` is a live object cell.
pub(super) unsafe fn member<W: Walk>(
    plan: &Plan<W>,
    v: AnyValue,
    g: AnyValue,
    at: &Field,
) -> Result<u64, Fail> {
    debug_assert_eq!(plan.u8(at.node), OPT);
    if is_undefined(g) && !unsafe { plan.owns(v, at) } {
        unsafe { plan.mark_absent(at) };
    }
    if plan.u8(at.node + 1) == NUM && is_null(g) {
        return unsafe { plan.park_null(at) };
    }
    unsafe { plan.field_value(at.node, g) }
}

impl<W: Walk> Plan<W> {
    /// A `null` the value carries for an optional `number` member: the
    /// slot takes its undefined sentinel and the `null` goes to the
    /// field's sidecar behind `FLAG_OBJ_EXOTIC_FIELD` (§10.1.9.2 [[Set]]
    /// on the fresh cell), where a write through the any lane would
    /// have put it. Answers the slot bits.
    ///
    /// # Safety
    /// As [`member`].
    unsafe fn park_null(&self, at: &Field) -> Result<u64, Fail> {
        unsafe {
            (at.cell.add(at.off) as *mut u64).write(torajs_rc::F64_UNDEF_SENTINEL_BITS);
        }
        let key = unsafe { __torajs_str_alloc(self.base.add(at.name), at.len as i64) };
        unsafe {
            crate::struct_field_ffi::exotic::__torajs_obj_field_set_exotic_impl(
                at.cell.cast(),
                key.cast(),
                0,
                0,
                1,
            );
            __torajs_str_drop(key.cast());
        }
        if unsafe { __torajs_throw_check() } != 0 {
            return Err(Fail::Thrown);
        }
        Ok(torajs_rc::F64_UNDEF_SENTINEL_BITS)
    }

    /// Whether `v` owns the property `at` names: §7.3.12
    /// HasOwnProperty, which tells a member the value lacks from one it
    /// holds as `undefined`.
    ///
    /// # Safety
    /// `v` is a live object cell.
    unsafe fn owns(&self, v: AnyValue, at: &Field) -> bool {
        let key = unsafe { __torajs_str_alloc(self.base.add(at.name), at.len as i64) };
        let has = unsafe { crate::prop_has::__torajs_any_own_prop_has(v, key.cast()) };
        unsafe { __torajs_str_drop(key.cast()) };
        has != 0
    }

    /// The optional member `at` is no own property of the cell: the
    /// value lacked it (§10.1.11 — `"a" in p` is false and the keys
    /// leave it out). Recorded the way a literal that leaves the member
    /// out records it: the header bit and the slot's bit in the absent
    /// set, or the deletion record for a slot the set cannot name.
    ///
    /// # Safety
    /// As [`member`].
    unsafe fn mark_absent(&self, at: &Field) {
        let slot = (at.off - OBJ_HEADER_SIZE) / 8;
        if slot < ABSENT_SET_SLOTS {
            unsafe {
                let flags = at.cell.add(OBJ_FLAGS_OFF) as *mut u16;
                flags.write(flags.read() | torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD);
                let set = at.cell.add(ABSENT_SET_OFF) as *mut u32;
                set.write(set.read() | 1 << slot);
            }
            return;
        }
        let key = unsafe { __torajs_str_alloc(self.base.add(at.name), at.len as i64) };
        unsafe {
            __torajs_obj_declared_field_delete(at.cell.cast(), key.cast());
            __torajs_str_drop(key.cast());
        }
    }
}
