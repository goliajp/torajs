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
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_to_bool};

unsafe extern "C" {
    fn __torajs_anyv_get_property_descriptor(obj_any: u64, key: *const c_void) -> u64;
    fn __torajs_anyv_is_extensible(obj_any: u64) -> bool;
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// The target's own property descriptor, as the invariant checks read
/// it. `value` is owned (undefined for an accessor).
pub(crate) struct TargetDesc {
    pub(crate) configurable: bool,
    /// `Some(writable)` for a data property, `None` for an accessor.
    pub(crate) writable: Option<bool>,
    pub(crate) value: AnyValue,
    pub(crate) get_is_undefined: bool,
    pub(crate) set_is_undefined: bool,
}

impl Drop for TargetDesc {
    fn drop(&mut self) {
        // SAFETY: `value` is the owned field read below.
        unsafe { __torajs_anyv_rc_dec(self.value) };
    }
}

/// Read one descriptor field — owned.
///
/// # Safety
/// `desc` is a live descriptor object.
unsafe fn field(desc: AnyValue, name: &[u8]) -> Result<AnyValue, ()> {
    unsafe { crate::proxy_get_prop::get_by_name(desc, name) }.ok_or(())
}

/// target.[[GetOwnProperty]](key). `Err` = a pending throw.
///
/// # Safety
/// `target` is a live object; `key` a live Str or Symbol cell.
pub(crate) unsafe fn target_desc(
    target: AnyValue,
    key: *const c_void,
) -> Result<Option<TargetDesc>, ()> {
    unsafe {
        let d = __torajs_anyv_get_property_descriptor(target, key);
        if __torajs_throw_check() != 0 {
            __torajs_anyv_rc_dec(d);
            return Err(());
        }
        if is_undefined(d) {
            return Ok(None);
        }
        let read = || -> Result<TargetDesc, ()> {
            let configurable = field(d, b"configurable")?;
            let writable = field(d, b"writable")?;
            let get = field(d, b"get")?;
            let set = field(d, b"set")?;
            let value = field(d, b"value")?;
            // A complete data descriptor carries `writable`; an
            // accessor descriptor carries `get` / `set` instead.
            let is_accessor = is_undefined(writable)
                && (!is_undefined(get) || !is_undefined(set) || is_undefined(value));
            let out = TargetDesc {
                configurable: __torajs_anyv_to_bool(configurable),
                writable: (!is_accessor).then(|| __torajs_anyv_to_bool(writable)),
                value,
                get_is_undefined: is_undefined(get),
                set_is_undefined: is_undefined(set),
            };
            for v in [configurable, writable, get, set] {
                __torajs_anyv_rc_dec(v);
            }
            Ok(out)
        };
        let out = read();
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
    if d.configurable {
        return Ok(());
    }
    match d.writable {
        Some(false) if !unsafe { crate::nanbox_ffi::__torajs_anyv_same_value(result, d.value) } => {
            violated(c"proxy get trap must report a non-writable, non-configurable property's value")
        }
        None if d.get_is_undefined && !is_undefined(result) => {
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
    if d.configurable {
        return Ok(());
    }
    match d.writable {
        Some(false) if !unsafe { crate::nanbox_ffi::__torajs_anyv_same_value(value, d.value) } => {
            violated(c"proxy set trap cannot change a non-writable, non-configurable property")
        }
        None if d.set_is_undefined => {
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
    if !d.configurable {
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
    if !d.configurable {
        return violated(c"proxy deleteProperty trap cannot delete a non-configurable property");
    }
    if !unsafe { target_extensible(target) }? {
        return violated(
            c"proxy deleteProperty trap cannot delete a property of a non-extensible target",
        );
    }
    Ok(())
}
