//! §6.2.6 Property Descriptor records, at value level, for the Proxy
//! internal methods.
//!
//! The ordinary define kernels in torajs-dynobj validate a descriptor
//! against the flag bits of a stored entry; a Proxy has no stored
//! entry. Its [[GetOwnProperty]] and [[DefineOwnProperty]] compare a
//! trap's answer with what the TARGET reports through reflection, so
//! both sides arrive as descriptor objects and the comparison is the
//! spec's own, field by field: §10.1.6.2 IsCompatiblePropertyDescriptor
//! is §10.1.6.3 ValidateAndApplyPropertyDescriptor with no object to
//! apply to, which is a pure function of two records.

use core::ffi::c_void;

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, box_bool, box_void_ptr, is_undefined};
use crate::nanbox_ffi::{__torajs_anyv_rc_dec, __torajs_anyv_same_value, __torajs_anyv_to_bool};

unsafe extern "C" {
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_dynobj_alloc() -> *mut c_void;
}

/// A Property Descriptor record. Every field is optional; the value
/// fields hold OWNED AnyValues.
#[derive(Default)]
pub(crate) struct Desc {
    pub(crate) value: Option<AnyValue>,
    pub(crate) writable: Option<bool>,
    pub(crate) get: Option<AnyValue>,
    pub(crate) set: Option<AnyValue>,
    pub(crate) enumerable: Option<bool>,
    pub(crate) configurable: Option<bool>,
}

impl Drop for Desc {
    fn drop(&mut self) {
        for v in [self.value, self.get, self.set].into_iter().flatten() {
            // SAFETY: every present value field is an owned AnyValue.
            unsafe { __torajs_anyv_rc_dec(v) };
        }
    }
}

impl Desc {
    /// §6.2.6.1 IsAccessorDescriptor.
    pub(crate) fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    /// §6.2.6.2 IsDataDescriptor.
    pub(crate) fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    /// §6.2.6.3 IsGenericDescriptor.
    fn is_generic(&self) -> bool {
        !self.is_accessor() && !self.is_data()
    }

    /// §6.2.6.6 CompletePropertyDescriptor.
    pub(crate) fn complete(&mut self) {
        if self.is_accessor() {
            self.get.get_or_insert(VALUE_UNDEFINED);
            self.set.get_or_insert(VALUE_UNDEFINED);
        } else {
            self.value.get_or_insert(VALUE_UNDEFINED);
            self.writable.get_or_insert(false);
        }
        self.enumerable.get_or_insert(false);
        self.configurable.get_or_insert(false);
    }

    /// §6.2.6.4 FromPropertyDescriptor — a fresh ordinary object
    /// carrying exactly the present fields, in the spec's order.
    ///
    /// # Safety
    /// The record's value fields are live.
    pub(crate) unsafe fn to_object(&self) -> AnyValue {
        unsafe {
            let mut obj = box_void_ptr(__torajs_dynobj_alloc());
            let fields: [(&[u8], Option<AnyValue>); 6] = [
                (b"value", self.value),
                (b"writable", self.writable.map(box_bool)),
                (b"get", self.get),
                (b"set", self.set),
                (b"enumerable", self.enumerable.map(box_bool)),
                (b"configurable", self.configurable.map(box_bool)),
            ];
            for (name, v) in fields {
                if let Some(v) = v {
                    put(&mut obj, name, v);
                }
            }
            obj
        }
    }
}

/// Store `v` (borrowed) under `name` on a fresh descriptor object.
unsafe fn put(obj: &mut AnyValue, name: &[u8], v: AnyValue) {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *mut c_void;
        let (tag, payload) = (
            crate::__torajs_anyv_unbox_tag(v),
            crate::__torajs_anyv_unbox_value(v),
        );
        crate::payload_rc_inc(tag, payload);
        crate::member_set::__torajs_any_member_set(obj, key, tag as u64, payload as u64, -1);
        __torajs_str_drop(key);
    }
}

/// HasProperty(obj, name) then Get(obj, name) — `Ok(None)` for an
/// absent field, `Err` for a pending throw.
unsafe fn field(obj: AnyValue, name: &[u8]) -> Result<Option<AnyValue>, ()> {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64) as *mut c_void;
        let has = crate::prop_has_chain::__torajs_any_has_property(obj, key);
        let out = if __torajs_throw_check() != 0 {
            Err(())
        } else if has == 0 {
            Ok(None)
        } else {
            crate::proxy_get_prop::get_by_key_checked(obj, key)
                .map(Some)
                .ok_or(())
        };
        __torajs_str_drop(key);
        out
    }
}

/// A boolean field, through ToBoolean.
unsafe fn bool_field(obj: AnyValue, name: &[u8]) -> Result<Option<bool>, ()> {
    let Some(v) = (unsafe { field(obj, name) })? else {
        return Ok(None);
    };
    let b = unsafe { __torajs_anyv_to_bool(v) };
    unsafe { __torajs_anyv_rc_dec(v) };
    Ok(Some(b))
}

/// A `get` / `set` field — §6.2.6.5 steps 11.b / 13.b: callable or
/// undefined.
unsafe fn accessor_field(obj: AnyValue, name: &[u8]) -> Result<Option<AnyValue>, ()> {
    let Some(v) = (unsafe { field(obj, name) })? else {
        return Ok(None);
    };
    if !is_undefined(v) && !is_callable(v) {
        unsafe { __torajs_anyv_rc_dec(v) };
        return type_error(c"property descriptor getter or setter must be callable or undefined");
    }
    Ok(Some(v))
}

/// IsCallable over the two callable shapes: a closure, and a Proxy
/// whose target is one.
pub(crate) fn is_callable(v: AnyValue) -> bool {
    crate::promise_capability::is_callable(v)
        || (crate::proxy::is_proxy(v) && unsafe { crate::proxy_callable::proxy_is_callable(v) })
}

fn type_error<T>(msg: &core::ffi::CStr) -> Result<T, ()> {
    // SAFETY: a static C string.
    unsafe { __torajs_throw_type_error(msg.as_ptr()) };
    Err(())
}

/// §6.2.6.5 ToPropertyDescriptor. `Err` = a pending throw.
///
/// # Safety
/// `obj` is a live AnyValue.
pub(crate) unsafe fn to_property_descriptor(obj: AnyValue) -> Result<Desc, ()> {
    if !crate::to_primitive::is_object_value(obj) {
        return type_error(c"property description must be an object");
    }
    let mut d = Desc::default();
    unsafe {
        d.enumerable = bool_field(obj, b"enumerable")?;
        d.configurable = bool_field(obj, b"configurable")?;
        d.value = field(obj, b"value")?;
        d.writable = bool_field(obj, b"writable")?;
        d.get = accessor_field(obj, b"get")?;
        d.set = accessor_field(obj, b"set")?;
    }
    if d.is_accessor() && d.is_data() {
        return type_error(
            c"invalid property descriptor: cannot both specify accessors and a value or writable attribute",
        );
    }
    Ok(d)
}

/// §10.1.6.2 IsCompatiblePropertyDescriptor(Extensible, Desc,
/// Current) — ValidateAndApplyPropertyDescriptor with O undefined.
/// `current`, when present, is a complete descriptor.
pub(crate) fn is_compatible(extensible: bool, desc: &Desc, current: Option<&Desc>) -> bool {
    let Some(cur) = current else {
        return extensible;
    };
    if cur.configurable != Some(false) {
        return true;
    }
    if desc.configurable == Some(true) {
        return false;
    }
    if desc.enumerable.is_some_and(|e| Some(e) != cur.enumerable) {
        return false;
    }
    if !desc.is_generic() && desc.is_accessor() != cur.is_accessor() {
        return false;
    }
    let differs = |d: Option<AnyValue>, c: Option<AnyValue>| {
        d.is_some_and(|d| !unsafe { __torajs_anyv_same_value(d, c.unwrap_or(VALUE_UNDEFINED)) })
    };
    if cur.is_accessor() {
        return !differs(desc.get, cur.get) && !differs(desc.set, cur.set);
    }
    if cur.writable == Some(false) {
        return desc.writable != Some(true) && !differs(desc.value, cur.value);
    }
    true
}
