//! `Object.getOwnPropertyDescriptor` on a plain DynObj receiver — the
//! last arm of the dispatch in [`crate::reflect_get_property_descriptor`],
//! moved here for file size. The receiver is a live `TAG_DYNOBJ` cell
//! and `key` a non-null property key cell.

use core::ffi::c_void;

use crate::reflect::{
    __torajs_accessor_get_getter, __torajs_accessor_get_setter, __torajs_dynobj_get_flags,
    __torajs_dynobj_get_tag, __torajs_dynobj_get_value, __torajs_dynobj_has, __torajs_rc_inc,
    ANY_ACCESSOR, ANY_HEAP, ANY_UNDEF, VALUE_UNDEFINED_IMM, build_accessor_descriptor,
    build_data_descriptor,
};
use crate::reflect_get_property_descriptor::{builtin_proto_descriptor, key_is_symbol_cell};

/// The dynobj's own descriptor for `key`, or a synthesized
/// prototype-method descriptor for a name it does not own.
///
/// # Safety
///
/// `dynobj` is a live `TAG_DYNOBJ` cell; `key` is a non-null key cell.
pub(crate) unsafe fn dynobj_descriptor(dynobj: *const c_void, key: *const c_void) -> u64 {
    let k_str = key as *const u8;
    if !unsafe { __torajs_dynobj_has(dynobj, k_str) } {
        // A missing symbol key stops here: `builtin_proto_descriptor`
        // synthesizes name-keyed prototype methods and reads the key's
        // Str payload to do it (see the §6.1.7 note above).
        if unsafe { key_is_symbol_cell(key) } {
            return VALUE_UNDEFINED_IMM;
        }
        return unsafe { builtin_proto_descriptor(dynobj, key) };
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k_str) };
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k_str) };
    let flags = unsafe { __torajs_dynobj_get_flags(dynobj, k_str) };

    // RFC C3 — accessor entry: report `{ get, set, enumerable,
    // configurable }` (no value/writable). `v_val` is the AccessorPair
    // pointer; the getter/setter closures are returned as ANY_HEAP
    // cells (the desc owns a fresh ref to each) or `undefined`.
    if v_tag == ANY_ACCESSOR {
        let pair = v_val as *const c_void;
        let getter = unsafe { __torajs_accessor_get_getter(pair) };
        let setter = unsafe { __torajs_accessor_get_setter(pair) };
        let (get_t, get_v) = if getter.is_null() {
            (ANY_UNDEF as u64, 0u64)
        } else {
            unsafe { __torajs_rc_inc(getter) };
            (ANY_HEAP as u64, getter as u64)
        };
        let (set_t, set_v) = if setter.is_null() {
            (ANY_UNDEF as u64, 0u64)
        } else {
            unsafe { __torajs_rc_inc(setter) };
            (ANY_HEAP as u64, setter as u64)
        };
        return unsafe {
            build_accessor_descriptor(
                get_t,
                get_v,
                set_t,
                set_v,
                (flags >> 1) & 1,
                (flags >> 2) & 1,
            )
        };
    }

    if v_tag as i64 == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer — the
        // source dynobj keeps its share, the descriptor owns a
        // fresh one.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    // desc owns rc=1 from dynobj_alloc; transferred to caller
    // via the returned cell-encoded AnyValue (pre-7d the AnyBox-
    // wrapped path rc_inc'd + dropped the local; both cancel
    // out and we skip both).
    unsafe { build_data_descriptor(v_tag, v_val, flags & 1, (flags >> 1) & 1, (flags >> 2) & 1) }
}
