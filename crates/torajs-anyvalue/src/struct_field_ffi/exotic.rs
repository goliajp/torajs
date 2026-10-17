//! The slow legs of a typed class-instance field access — what the
//! inline load or store runs instead once the instance's header says
//! its own property set is no longer what the layout describes
//! (`FLAG_FROZEN` / `FLAG_OBJ_EXOTIC_FIELD`).
//!
//! A redefined declared field may have become an accessor, been
//! demoted to non-writable, been deleted, or be parked in its sidecar
//! as a value its typed slot cannot hold. The any lane already answers
//! every one of those against the sidecar, so both legs are that lane
//! called on the boxed cell: §10.1.8.1 [[Get]] for a read, §10.1.9.2
//! [[Set]] for a write. They are reached through the link-judged
//! seams in torajs-dispatch, stubbed when nothing in the artifact can
//! raise the header bits.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, as_void_ptr, box_void_ptr, is_cell, is_null, is_undefined};

/// §10.1.8.1 [[Get]] of `name` on the class instance `p`, the result
/// OWNED (a getter's return carries its own reference; a data answer
/// takes one). A getter's throw stays pending for the caller's check.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell; `name` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_get_exotic_impl(
    p: *mut c_void,
    name: *const c_void,
) -> AnyValue {
    let recv = box_void_ptr(p);
    unsafe { crate::member_get_receiver::__torajs_any_member_get_with_receiver(recv, name, recv) }
}

unsafe extern "C" {
    fn __torajs_obj_proto_borrowed(cell: *const c_void) -> u64;
}

/// What the declared accessor member `name` of the instance `p` is
/// now: 0 while it is an own accessor, 1 once deleted (its deletion
/// record is set, §10.1.10), 2 once replaced by the ordinary property
/// a store after the delete created. A typed read, store or spread of
/// an object-literal accessor a delete of the program can reach asks
/// it before calling the accessor in the layout slot.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell; `name` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_member_deleted(p: *mut c_void, name: *const c_void) -> i64 {
    use crate::struct_probe::AccessorState;
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(name) };
    match unsafe { crate::struct_probe::accessor_state(p, k.as_bytes()) } {
        AccessorState::Live => 0,
        AccessorState::Deleted => 1,
        AccessorState::Replaced => 2,
    }
}

/// §10.1.8.1 [[Get]] of the deleted declared member `name` of the
/// instance `p`, OWNED: no own property is left (§10.1.10), so
/// OrdinaryGet asks the prototype (step 3), `p` staying the receiver.
/// An instance whose prototype nothing registered or reassigned has
/// %Object.prototype%; an explicit null prototype ends the chain.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell; `name` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_deleted_member_get(
    p: *mut c_void,
    name: *const c_void,
) -> AnyValue {
    // a replaced member's own property is the dict entry
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(name) };
    if unsafe { crate::struct_probe::accessor_state(p, k.as_bytes()) }
        == crate::struct_probe::AccessorState::Replaced
    {
        return unsafe { __torajs_obj_field_get_exotic_impl(p, name) };
    }
    let root = unsafe { __torajs_obj_proto_borrowed(p) };
    let proto = if is_cell(root) {
        root
    } else if unsafe { crate::member_get_layout::obj_user_proto_raised(p) } {
        return crate::nanbox::VALUE_UNDEFINED;
    } else {
        use torajs_rc::builtin_proto::{__torajs_get_builtin_prototype, OBJECT_PROTO_TAG};
        let op = unsafe { __torajs_get_builtin_prototype(OBJECT_PROTO_TAG as i64) };
        if op.is_null() {
            return crate::nanbox::VALUE_UNDEFINED;
        }
        box_void_ptr(op)
    };
    let get = crate::member_get_receiver::__torajs_any_member_get_with_receiver;
    unsafe { get(proto, name, box_void_ptr(p)) }
}

/// §10.1.9.2 [[Set]] of `name` on the class instance `p` to the
/// payload `(tag, value)`, whose stake transfers in. `strict` picks
/// the §13.15.2 goal: a refusal records the TypeError when non-zero
/// and answers 0 silently otherwise. Answers 1 when the payload was
/// written or a setter ran.
///
/// # Safety
/// `p` is a live `Tag::Obj` cell; `name` is a live Str cell;
/// `(tag, value)` is the caller's owned payload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_set_exotic_impl(
    p: *mut c_void,
    name: *mut c_void,
    tag: u64,
    value: u64,
    strict: i64,
) -> i64 {
    let mut recv = box_void_ptr(p);
    unsafe { crate::member_set::any_member_set_impl(&mut recv, name, tag, value, -1, strict != 0) }
}

/// A getter's answer `v` read through a declared `number` field whose
/// annotation admits `undefined` (`v?: number`): the number, or the
/// slot's undefined sentinel for `undefined`. Anything else records the
/// TypeError `site` spells and answers NaN.
///
/// # Safety
/// `site` is a live Str literal block; a cell payload of `v` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_exotic_number_or_undef(
    v: AnyValue,
    site: *const u8,
) -> f64 {
    if is_undefined(v) {
        return f64::from_bits(torajs_rc::F64_UNDEF_SENTINEL_BITS);
    }
    unsafe { crate::anyv_cast::__torajs_anyv_cast_number(v, site) }
}

/// A getter's answer `v` read through a declared field whose typed
/// lane spells its cells its own way (a closure, a typed collection):
/// only `slot`, the cell the field already holds, crosses, with a
/// reference of its own; a nullish answer crosses as an empty slot.
/// Anything else records the TypeError `site` spells and answers NULL.
///
/// # Safety
/// `slot` is NULL or a live cell; `site` is a live Str literal block; a
/// cell payload of `v` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_exotic_same_cell(
    v: AnyValue,
    slot: *mut c_void,
    site: *const u8,
) -> *mut c_void {
    if slot.is_null() && (is_undefined(v) || is_null(v)) {
        return slot;
    }
    if !slot.is_null() && is_cell(v) && as_void_ptr(v) == slot {
        unsafe { torajs_rc::__torajs_rc_inc(slot) };
        return slot;
    }
    unsafe { crate::anyv_cast::reject(v, site) };
    core::ptr::null_mut()
}

/// A getter's answer `v` read through a declared field whose slot holds
/// a raw function address: no value of the any lane is that address, so
/// every answer records the TypeError `site` spells. Answers NULL.
///
/// # Safety
/// `site` is a live Str literal block; a cell payload of `v` is live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_obj_field_exotic_refuse(
    v: AnyValue,
    site: *const u8,
) -> *mut c_void {
    unsafe { crate::anyv_cast::reject(v, site) };
    core::ptr::null_mut()
}
