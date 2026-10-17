//! The per-slot half of [`super::write_struct`] — which own property a
//! layout slot stands for, and its value. Split out of the parent
//! under the 500-line file discipline; a child module reaches its
//! parent's private items, so the externs stay where they are.

use core::ffi::c_void;

use super::*;

unsafe extern "C" {
    /// torajs-meta — is this declared member deleted?
    fn __torajs_obj_field_is_deleted(cell: *const c_void, name: *const u8, name_len: u32) -> i64;
}

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
/// size limit. `Threw` = a getter left a pending throw and the walk
/// stops.
pub(super) enum SlotEntry {
    Threw,
    Emit {
        key: StructFieldName,
        value: u64,
        owned: bool,
    },
}

/// The own property a layout slot stands for.
pub(super) struct SlotKey {
    ptr: *const u8,
    len: usize,
    /// An accessor slot (`__getter_p` stands for `p`); otherwise the
    /// key is the data field's own slot name.
    accessor: bool,
}

/// The property key slot `i` serializes under, or `None` when the slot
/// contributes no entry: an error's `message` / `stack`, an unassigned
/// error `name`, the second slot of a get/set pair.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `layout` its live layout entry;
/// `i` is below the entry's field count.
pub(super) unsafe fn struct_slot_key(
    ptr: *mut c_void,
    layout: *const c_void,
    i: u32,
    is_error: bool,
) -> Option<SlotKey> {
    unsafe {
        let name = __torajs_struct_field_name(layout, i);
        if is_error {
            let key = core::slice::from_raw_parts(name.ptr, name.len);
            if key == b"message" || key == b"stack" {
                return None;
            }
            // §20.5.3.2 — an unassigned `name` is not an own property
            // at all (the class name is the prototype's), so it
            // contributes no entry. An assigned one is ordinary and
            // enumerable, and serializes like any other field.
            if key == b"name" && crate::struct_error_msg::error_name_is_absent(ptr) {
                return None;
            }
        }
        // An accessor rides the layout under a synthetic slot name, so
        // the raw name is not the property's; §25.5.2 walks
        // EnumerableOwnProperties, which is what the keys walker
        // answers, and this takes the same slot-name-to-property-name
        // step it does.
        let (key_ptr, key_len) = slot_property_name(name.ptr, name.len);
        let accessor = key_len != name.len;
        // A get/set pair is two slots and one own property.
        if accessor && accessor_pair_already_emitted(layout, i, key_ptr, key_len) {
            return None;
        }
        Some(SlotKey {
            ptr: key_ptr,
            len: key_len,
            accessor,
        })
    }
}

/// Whether the property `k` names is own and enumerable by the
/// instance's redefine / delete / absent records, asked with the
/// PROPERTY name: a redefine of an accessor records against `p`, never
/// `__getter_p`. A deletion
/// record clears the entry's attribute bits and an absent member reads
/// like one, so the one question covers all three. Runs no user code,
/// so [`super::write_struct`] can take the whole of K (§25.5.2.4 step
/// 6) before the first getter can move it.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `k` is [`struct_slot_key`]'s
/// answer for one of its slots.
pub(super) unsafe fn struct_key_listed(ptr: *mut c_void, k: &SlotKey) -> bool {
    // an accessor a store replaced is listed by the expando walk
    unsafe {
        __torajs_obj_field_is_nonenumerable(ptr, k.ptr, k.len as u32) == 0
            && !(k.accessor
                && crate::struct_probe::accessor_state(
                    ptr,
                    core::slice::from_raw_parts(k.ptr, k.len),
                ) == crate::struct_probe::AccessorState::Replaced)
    }
}

/// `? Get(holder, key)` for a key [`struct_slot_key`] put in K.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `k` is [`struct_slot_key`]'s
/// answer for one of its slots.
pub(super) unsafe fn struct_slot_value(ptr: *mut c_void, k: SlotKey) -> SlotEntry {
    unsafe {
        let key = StructFieldName {
            ptr: k.ptr,
            len: k.len,
        };
        // A data member answers its slot unless it was deleted or
        // redefined as an accessor (the read sees both records); an
        // accessor answers its getter while no member of the instance
        // was redefined, deleted or shadowed.
        if !k.accessor {
            let mut value: u64 = 0;
            if __torajs_struct_field_read_anyv(ptr, k.ptr, k.len as u32, &mut value) != 0 {
                return SlotEntry::Emit {
                    key,
                    value,
                    owned: false,
                };
            }
        } else if !crate::member_get::header_flag(ptr, torajs_rc::FLAG_OBJ_EXOTIC_FIELD) {
            let value = crate::struct_probe::__torajs_struct_accessor_get(ptr, k.ptr, k.len as u32);
            return threw_or_emit(key, value);
        }
        threw_or_emit(key, exotic_get(ptr, k.ptr, k.len))
    }
}

/// The full `? Get(holder, key)` of a member of K the plain reads
/// cannot answer: a member deleted since K was taken has no own face
/// left, so the prototype chain answers — the own-face probe would
/// still find a deleted accessor's getter in the layout — and any
/// other answers through the ordinary [[Get]], which reads a
/// redefinition's sidecar. Owned.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell; `key_ptr` points at `key_len`
/// readable bytes.
unsafe fn exotic_get(ptr: *mut c_void, key_ptr: *const u8, key_len: usize) -> u64 {
    unsafe {
        let cell = __torajs_str_alloc(key_ptr, key_len as i64) as *const c_void;
        let recv = box_void_ptr(ptr);
        let value = match __torajs_obj_field_is_deleted(ptr, key_ptr, key_len as u32) {
            0 => super::super::replacer::get_owned(recv, cell),
            _ => {
                let (tag, raw) = crate::struct_error_msg::struct_proto_chain_pair(ptr, cell);
                crate::len_get::box_probe_pair(tag, raw, recv, cell)
            }
        };
        __torajs_str_drop(cell as *mut c_void);
        value
    }
}

/// An OWNED [[Get]] result, or the pending throw it left.
///
/// # Safety
/// `value` is a live owned AnyValue.
unsafe fn threw_or_emit(key: StructFieldName, value: u64) -> SlotEntry {
    if unsafe { __torajs_throw_check() } != 0 {
        unsafe { crate::nanbox_ffi::__torajs_anyv_rc_dec(value) };
        return SlotEntry::Threw;
    }
    SlotEntry::Emit {
        key,
        value,
        owned: true,
    }
}
