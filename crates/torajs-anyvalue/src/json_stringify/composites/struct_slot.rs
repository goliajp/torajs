//! The per-slot half of [`super::write_struct`] — which own property a
//! layout slot stands for, and its value. Split out of the parent
//! under the 500-line file discipline; a child module reaches its
//! parent's private items, so the externs stay where they are.

use core::ffi::c_void;

use super::*;

/// The own-property key a layout slot serializes under: an accessor
/// slot answers the property it stands for, a data field answers
/// itself. The twin of `torajs-meta::struct_reflect::slot_key`, which
/// is what `Object.keys` walks — both ask torajs-structmeta for the
/// spelling rather than either one holding it.
///
/// # Safety
/// `ptr` is NULL or points at `len` readable bytes.
unsafe fn slot_property_name(ptr: *const u8, len: usize) -> (*const u8, usize) {
    if ptr.is_null() {
        return (ptr, len);
    }
    let off = unsafe { __torajs_accessor_slot_prefix_len(ptr, len as u32) } as usize;
    if off == 0 || off >= len {
        return (ptr, len);
    }
    (unsafe { ptr.add(off) }, len - off)
}

/// Whether an earlier slot already serialized `key` — a get/set pair
/// occupies two layout slots and is one own property. `Object.keys`'s
/// walker dedups the same way (`struct_enum::key_already_emitted`).
///
/// # Safety
/// `layout` is a live layout entry; `key` points at `key_len`
/// readable bytes.
unsafe fn accessor_pair_already_emitted(
    layout: *const c_void,
    upto: u32,
    key: *const u8,
    key_len: usize,
) -> bool {
    let want = unsafe { core::slice::from_raw_parts(key, key_len) };
    for j in 0..upto {
        let prev = unsafe { __torajs_struct_field_name(layout, j) };
        let (p, plen) = unsafe { slot_property_name(prev.ptr, prev.len) };
        if plen == key_len && unsafe { core::slice::from_raw_parts(p, plen) } == want {
            return true;
        }
    }
    false
}

/// What one layout slot contributes to `SerializeJSONObject` —
/// [`super::write_struct`]'s per-slot half, split out at the file's function
/// size limit. `Skip` = not an own enumerable property; `Threw` = a
/// getter left a pending throw and the walk stops.
pub(super) enum SlotEntry {
    Skip,
    Threw,
    Emit {
        key: StructFieldName,
        value: u64,
        owned: bool,
    },
}

/// See [`SlotEntry`].
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `layout` its live layout entry;
/// `i` is below the entry's field count.
pub(super) unsafe fn struct_slot_entry(
    ptr: *mut c_void,
    layout: *const c_void,
    i: u32,
    is_error: bool,
) -> SlotEntry {
    unsafe {
        let name = __torajs_struct_field_name(layout, i);
        if is_error {
            let key = core::slice::from_raw_parts(name.ptr, name.len);
            if key == b"message" || key == b"stack" {
                return SlotEntry::Skip;
            }
            // §20.5.3.2 — an unassigned `name` is not an own property
            // at all (the class name is the prototype's), so it
            // contributes no entry. An assigned one is ordinary and
            // enumerable, and serializes like any other field.
            if key == b"name" && crate::struct_error_msg::error_name_is_absent(ptr) {
                return SlotEntry::Skip;
            }
        }
        // An accessor rides the layout under a synthetic slot name, so
        // the raw name is not the property's — and the read below,
        // which correctly refuses that spelling, answered 0 and
        // dropped the whole property. `{a: 1, get b() { return 2 }}`
        // serialized as `{"a":1}` through the any lane while
        // `Object.keys` on the very same cell listed `b`. §25.5.2
        // walks EnumerableOwnProperties, which is what the keys walker
        // answers; this one takes the same two steps it does — slot
        // name to property name, and [[Get]] for the value.
        let (key_ptr, key_len) = slot_property_name(name.ptr, name.len);
        let key = StructFieldName {
            ptr: key_ptr,
            len: key_len,
        };
        // RFC 20260806-declared-field-redefine — the same filter one
        // line up, for an enumerability USER code moved rather than
        // the spec fixing. Free on any instance never passed to
        // defineProperty (the kernel gates on a header bit).
        //
        // Asked with the PROPERTY name, not the slot name: a redefine
        // of an accessor records against `p`, never `__getter_p`, so
        // asking the raw name here answered "still enumerable" about a
        // property `Object.keys` had already dropped.
        if __torajs_obj_field_is_nonenumerable(ptr, key_ptr, key_len as u32) != 0 {
            return SlotEntry::Skip;
        }
        if key_len == name.len {
            let mut value: u64 = 0;
            if __torajs_struct_field_read_anyv(ptr, name.ptr, name.len as u32, &mut value) == 0 {
                // A field redefined as an accessor is no data slot any
                // more; its getter answers (§25.5.2 walks [[Get]]).
                let bytes = core::slice::from_raw_parts(name.ptr, name.len);
                let Some(value) = crate::struct_probe::declared_accessor_get_bytes(ptr, bytes)
                else {
                    return SlotEntry::Skip;
                };
                if __torajs_throw_check() != 0 {
                    crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
                    return SlotEntry::Threw;
                }
                return SlotEntry::Emit {
                    key,
                    value,
                    owned: true,
                };
            }
            return SlotEntry::Emit {
                key,
                value,
                owned: false,
            };
        }
        // A get/set pair is two slots and one own property.
        if accessor_pair_already_emitted(layout, i, key_ptr, key_len) {
            return SlotEntry::Skip;
        }
        let value = crate::struct_probe::__torajs_struct_accessor_get(ptr, key_ptr, key_len as u32);
        if __torajs_throw_check() != 0 {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(value);
            return SlotEntry::Threw;
        }
        SlotEntry::Emit {
            key,
            value,
            owned: true,
        }
    }
}
