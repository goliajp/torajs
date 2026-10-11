//! §21.4.4.37 `Date.prototype.toJSON` on an arbitrary receiver (the
//! `.call`-re-dispatched reified cell); the Date-tagged arm itself is
//! in `method_call_date`.

use core::ffi::c_void;

use torajs_rc::{ANY_METHOD_TO_ISO_STRING, AnySlotTag};

use crate::nanbox::AnyValue;

unsafe extern "C" {
    /// torajs-throw — pending-throw probe for the ToPrimitive leg
    /// of the generic toJSON body below.
    fn __torajs_throw_check() -> i64;
    /// torajs-dynobj — own-property probe for the Invoke(O,
    /// "toISOString") leg (per-module extern convention, mirrors
    /// `method_call_object_proto`'s pair).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
}

/// §21.4.4.37 Date.prototype.toJSON through a `.call`-re-dispatched
/// reified cell — receiver-generic: `tv = ToPrimitive(O, number)`;
/// a non-finite Number tv answers null WITHOUT touching
/// `toISOString` (the t262 non-finite family plants a throwing
/// getter there); otherwise Invoke(O, "toISOString") re-enters the
/// dispatcher by mid. Recorded edge: the re-entry passes no name
/// Str, so a non-Date receiver's OWN `toISOString` property is not
/// consulted — mid-routed builtin arms only (the t262 non-finite
/// cases never reach the invoke leg).
pub(crate) unsafe fn date_to_json_generic(recv: AnyValue) -> AnyValue {
    unsafe {
        let is_obj = crate::nanbox::is_cell(recv) && crate::to_primitive::is_object_value(recv);
        let (tv, owned) = if is_obj {
            match crate::to_primitive::heap_to_primitive(crate::nanbox::as_void_ptr(recv), false) {
                Some(v) => (v, true),
                // Both ToPrimitive methods answered objects — the
                // TypeError is already recorded.
                None => return crate::nanbox::VALUE_UNDEFINED,
            }
        } else {
            (recv, false)
        };
        if __torajs_throw_check() != 0 {
            if owned {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(tv);
            }
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let non_finite = crate::nanbox::is_double(tv) && !crate::nanbox::as_double(tv).is_finite();
        if owned {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(tv);
        }
        if non_finite {
            return crate::nanbox::VALUE_NULL;
        }
        // §21.4.4.37 step 3 is Invoke(O, "toISOString") — an ordinary
        // Get-then-Call, so a plain object's OWN `toISOString` wins
        // over the builtin (t262 invoke-result hands `{ toISOString:
        // function () { … } }` to the reified cell). Same hand as
        // `arr_to_string_borrowed`'s `Get(this, "join")` leg; a
        // non-closure or missing entry falls through to the
        // redispatch, whose not-a-function TypeError is the same
        // answer §7.3.21 gives an uncallable Get result.
        if let Some((ptr, tag)) = crate::member_get::recv_cell(recv)
            && tag == torajs_rc::Tag::DynObj as u16
        {
            let key = {
                let bytes = b"toISOString";
                let s = crate::__torajs_str_alloc_pooled(bytes.len() as u64);
                core::ptr::copy_nonoverlapping(bytes.as_ptr(), s.add(16), bytes.len());
                s
            };
            let jtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let jval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            crate::__torajs_str_drop(key as *mut c_void);
            // Heap-tagged or nothing — see the twin in
            // `arr_to_string_borrowed`: any other tag's payload is a
            // value, not an address, and
            // `Date.prototype.toJSON.call({ toISOString: true })`
            // dereferenced it.
            if jtag == AnySlotTag::Heap as u64
                && let Some((env, entry)) =
                    crate::method_call::closure_cell_entry(jval as *mut c_void)
            {
                return crate::method_call::invoke_with_this(
                    env,
                    entry,
                    recv,
                    core::ptr::null(),
                    0,
                );
            }
        }
        crate::method_call::any_method_redispatch(
            recv,
            ANY_METHOD_TO_ISO_STRING,
            core::ptr::null(),
            0,
        )
    }
}
