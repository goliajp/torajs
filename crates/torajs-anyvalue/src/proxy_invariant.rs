//! The §10.5 invariant checks a Proxy runs over its trap results.
//!
//! A handler may answer anything, but not something that contradicts
//! what the target has already committed to: a non-configurable
//! property cannot be reported missing, a non-writable one cannot
//! change value, a non-extensible target cannot grow or lose keys.
//! Each check needs the target's [[GetOwnProperty]] and, for the
//! extensibility rules, IsExtensible(target) — both ask the ordinary
//! reflection kernels, which recurse into a proxy target's own traps.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, is_undefined};
use crate::nanbox_ffi::__torajs_anyv_rc_dec;
use crate::proxy_descriptor::{Desc, is_compatible, to_property_descriptor};

unsafe extern "C" {
    fn __torajs_anyv_get_property_descriptor(obj_any: u64, key: *const c_void) -> u64;
    fn __torajs_anyv_is_extensible(obj_any: u64) -> bool;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// target.[[GetOwnProperty]](key), as a complete record. `Err` = a
/// pending throw.
///
/// # Safety
/// `target` is a live object; `key` a live Str or Symbol cell.
pub(crate) unsafe fn target_desc(target: AnyValue, key: *const c_void) -> Result<Option<Desc>, ()> {
    unsafe {
        let d = __torajs_anyv_get_property_descriptor(target, key);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(d);
            return Err(());
        }
        if is_undefined(d) {
            return Ok(None);
        }
        let out = to_property_descriptor(d);
        __torajs_anyv_rc_dec(d);
        out.map(Some)
    }
}

/// IsExtensible(target). `Err` = a pending throw.
///
/// # Safety
/// `target` is a live object.
pub(crate) unsafe fn target_extensible(target: AnyValue) -> Result<bool, ()> {
    unsafe {
        let b = __torajs_anyv_is_extensible(target);
        if __torajs_throw_check() != 0 {
            return Err(());
        }
        Ok(b)
    }
}

/// Record the invariant TypeError and answer the caller's `Err`.
pub(crate) fn violated<T>(msg: &core::ffi::CStr) -> Result<T, ()> {
    // SAFETY: a static C string.
    unsafe { __torajs_throw_type_error(msg.as_ptr()) };
    Err(())
}

/// §10.5.8 steps 9-10 — a `get` trap result against the target.
///
/// # Safety
/// `target` live; `key` a live key cell; `result` a live AnyValue.
pub(crate) unsafe fn check_get(
    target: AnyValue,
    key: *const c_void,
    result: AnyValue,
) -> Result<(), ()> {
    let Some(d) = (unsafe { target_desc(target, key) })? else {
        return Ok(());
    };
    if d.configurable != Some(false) {
        return Ok(());
    }
    let value = d.value.unwrap_or(crate::nanbox::VALUE_UNDEFINED);
    match d.writable {
        Some(false) if !unsafe { crate::nanbox_ffi::__torajs_anyv_same_value(result, value) } => {
            violated(c"proxy get trap must report a non-writable, non-configurable property's value")
        }
        None if undefined_field(d.get) && !is_undefined(result) => {
            violated(c"proxy get trap must report undefined for a non-configurable accessor without a getter")
        }
        _ => Ok(()),
    }
}

/// §10.5.9 steps 10-11 — a `set` trap that answered true.
///
/// # Safety
/// `target` live; `key` a live key cell; `value` a live AnyValue.
pub(crate) unsafe fn check_set(
    target: AnyValue,
    key: *const c_void,
    value: AnyValue,
) -> Result<(), ()> {
    let Some(d) = (unsafe { target_desc(target, key) })? else {
        return Ok(());
    };
    if d.configurable != Some(false) {
        return Ok(());
    }
    let current = d.value.unwrap_or(crate::nanbox::VALUE_UNDEFINED);
    match d.writable {
        Some(false) if !unsafe { crate::nanbox_ffi::__torajs_anyv_same_value(value, current) } => {
            violated(c"proxy set trap cannot change a non-writable, non-configurable property")
        }
        None if undefined_field(d.set) => {
            violated(c"proxy set trap cannot set a non-configurable accessor without a setter")
        }
        _ => Ok(()),
    }
}

/// §10.5.7 step 9 — a `has` trap that answered false.
///
/// # Safety
/// `target` live; `key` a live key cell.
pub(crate) unsafe fn check_has_false(target: AnyValue, key: *const c_void) -> Result<(), ()> {
    let Some(d) = (unsafe { target_desc(target, key) })? else {
        return Ok(());
    };
    if d.configurable == Some(false) {
        return violated(c"proxy has trap cannot hide a non-configurable property");
    }
    if !unsafe { target_extensible(target) }? {
        return violated(c"proxy has trap cannot hide a property of a non-extensible target");
    }
    Ok(())
}

/// §10.5.10 steps 10-13 — a `deleteProperty` trap that answered true.
///
/// # Safety
/// `target` live; `key` a live key cell.
pub(crate) unsafe fn check_delete_true(target: AnyValue, key: *const c_void) -> Result<(), ()> {
    let Some(d) = (unsafe { target_desc(target, key) })? else {
        return Ok(());
    };
    if d.configurable == Some(false) {
        return violated(c"proxy deleteProperty trap cannot delete a non-configurable property");
    }
    if !unsafe { target_extensible(target) }? {
        return violated(
            c"proxy deleteProperty trap cannot delete a property of a non-extensible target",
        );
    }
    Ok(())
}

/// An absent or undefined `get` / `set` field.
fn undefined_field(f: Option<AnyValue>) -> bool {
    f.is_none_or(is_undefined)
}

/// §10.5.5 steps 8-15 — a `getOwnPropertyDescriptor` trap result
/// (an object or undefined) against the target. Answers the trap's
/// descriptor as a complete record.
///
/// # Safety
/// `target` live; `key` a live key cell; `result` a live AnyValue.
pub(crate) unsafe fn check_get_own(
    target: AnyValue,
    key: *const c_void,
    result: AnyValue,
) -> Result<Option<Desc>, ()> {
    let target_desc = unsafe { target_desc(target, key) }?;
    if is_undefined(result) {
        let Some(td) = target_desc else {
            return Ok(None);
        };
        if td.configurable == Some(false) {
            return violated(
                c"proxy getOwnPropertyDescriptor trap cannot report a non-configurable property as non-existent",
            );
        }
        if !unsafe { target_extensible(target) }? {
            return violated(
                c"proxy getOwnPropertyDescriptor trap cannot report a property of a non-extensible target as non-existent",
            );
        }
        return Ok(None);
    }
    let extensible = unsafe { target_extensible(target) }?;
    let mut rd = unsafe { to_property_descriptor(result) }?;
    rd.complete();
    if !is_compatible(extensible, &rd, target_desc.as_ref()) {
        return violated(
            c"proxy getOwnPropertyDescriptor trap reported a descriptor incompatible with the target",
        );
    }
    if rd.configurable == Some(false) {
        let td_configurable = target_desc.as_ref().and_then(|td| td.configurable);
        if td_configurable != Some(false) {
            return violated(
                c"proxy getOwnPropertyDescriptor trap cannot report a configurable or missing property as non-configurable",
            );
        }
        if rd.writable == Some(false)
            && target_desc.as_ref().and_then(|td| td.writable) == Some(true)
        {
            return violated(
                c"proxy getOwnPropertyDescriptor trap cannot report a writable property as non-configurable and non-writable",
            );
        }
    }
    Ok(Some(rd))
}

/// §10.5.6 steps 10-16 — a `defineProperty` trap that answered true.
///
/// # Safety
/// `target` live; `key` a live key cell.
pub(crate) unsafe fn check_define_true(
    target: AnyValue,
    key: *const c_void,
    desc: &Desc,
) -> Result<(), ()> {
    let target_desc = unsafe { target_desc(target, key) }?;
    let extensible = unsafe { target_extensible(target) }?;
    let setting_config_false = desc.configurable == Some(false);
    let Some(td) = target_desc else {
        if !extensible {
            return violated(
                c"proxy defineProperty trap cannot add a property to a non-extensible target",
            );
        }
        if setting_config_false {
            return violated(
                c"proxy defineProperty trap cannot define a non-configurable property the target does not have",
            );
        }
        return Ok(());
    };
    if !is_compatible(extensible, desc, Some(&td)) {
        return violated(
            c"proxy defineProperty trap accepted a descriptor incompatible with the target",
        );
    }
    if setting_config_false && td.configurable == Some(true) {
        return violated(
            c"proxy defineProperty trap cannot define a non-configurable property the target has as configurable",
        );
    }
    if td.is_data()
        && td.configurable == Some(false)
        && td.writable == Some(true)
        && desc.writable == Some(false)
    {
        return violated(
            c"proxy defineProperty trap cannot make a writable non-configurable property non-writable",
        );
    }
    Ok(())
}
