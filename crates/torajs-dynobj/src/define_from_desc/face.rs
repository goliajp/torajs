//! §6.2.6.5 ToPropertyDescriptor — the accessor half of the
//! runtime-descriptor walk, split out of `define_from_desc.rs`
//! (file-size HARD RULE: the knife-5 `recv_kind_bit` helper pushed
//! the parent past 500). Same-crate child module — every private
//! parent item stays reachable through `super::`.

use core::ffi::c_void;

use super::{
    __torajs_anyv_cell_ptr, __torajs_anyv_to_bool, __torajs_anyv_unbox_tag, __torajs_rc_inc,
    __torajs_throw_type_error, __torajs_value_drop_heap, DescStore, desc_field, desc_field_get,
    release_desc_field,
};
use crate::define::define_apply;
use crate::layout::{
    ANY_HEAP, ANY_UNDEF, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
};

/// `FLAG_CLOSURE_RECV_FIRST` mirror (torajs-rc `flags.rs`) — the
/// header bit a promoted fn-expr face carries.
const FLAG_CLOSURE_RECV_FIRST: u16 = 1 << 12;

/// The `ACC_KIND_RECV` bit for a face closure that wants the
/// receiver in argv[0]; 0 for NULL / flag-less faces.
unsafe fn recv_kind_bit(face: *mut c_void) -> u64 {
    if face.is_null() {
        return 0;
    }
    let flags = unsafe { (face.cast::<u8>().add(6) as *const u16).read() };
    if flags & FLAG_CLOSURE_RECV_FIRST != 0 {
        crate::accessor::ACC_KIND_RECV as u64
    } else {
        0
    }
}

/// One accessor field (`get` / `set`) off [`desc_field_get`]'s
/// `(anyv, owned)` pair, normalized to an **owned** closure ref:
/// absent / explicit `undefined` answer NULL; a Closure cell answers
/// its pointer with one transferred ref (borrows inc, getter
/// products transfer as-is); anything else is the §6.2.6.5
/// ToPropertyDescriptor "not callable" TypeError (`Err`, input
/// released).
unsafe fn take_accessor_closure(
    field: Option<(u64, bool)>,
    which: &str,
) -> Result<*mut c_void, ()> {
    let Some((anyv, owned)) = field else {
        return Ok(core::ptr::null_mut());
    };
    if unsafe { __torajs_anyv_unbox_tag(anyv) } as u64 == ANY_UNDEF {
        return Ok(core::ptr::null_mut());
    }
    // Closure heap cell — universal header type_tag at +4 (Tag::Closure
    // = 3). The cell gate keeps a ShortStr field on the not-callable
    // path without materializing (the pre-fix `unbox_value` minted an
    // owned Str the throw leg never reclaimed).
    let val = unsafe { __torajs_anyv_cell_ptr(anyv) } as u64;
    if val != 0 {
        let type_tag = unsafe { *((val as *const u8).add(4) as *const u16) };
        if type_tag == 3 {
            if !owned {
                unsafe { __torajs_rc_inc(val as *mut c_void) };
            }
            return Ok(val as *mut c_void);
        }
    }
    unsafe { release_desc_field(anyv, owned) };
    unsafe {
        if which == "get" {
            __torajs_throw_type_error(c"Getter must be a function.".as_ptr() as *const u8);
        } else {
            __torajs_throw_type_error(c"Setter must be a function.".as_ptr() as *const u8);
        }
    }
    Err(())
}

/// §6.2.6.5 ToPropertyDescriptor — the accessor half. Returns true
/// when the descriptor named a get / set face: the entry is then
/// already defined (or the step 9/10 mix rejection already threw)
/// and the caller has nothing left to do. `flags_byte` is local
/// because the data half below starts from zero either way.
pub(super) unsafe fn try_define_accessor(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: DescStore,
) -> bool {
    let mut flags_byte: u64 = 0;
    // §6.2.6.5 ToPropertyDescriptor — every field read goes through
    // [[Get]] (a field that is itself an accessor invokes its
    // getter); presence checks use the raw entry probe.
    let get_f = unsafe { desc_field_get(desc, "get") };
    let set_f = unsafe { desc_field_get(desc, "set") };
    let (get_present, set_present) = (get_f.is_some(), set_f.is_some());
    if get_f.is_some() || set_f.is_some() {
        // §6.2.6.5 step 9/10 mix rejection — bun-parity messages
        // ('value' wins when both are present, matching JSC).
        let value_present = unsafe { desc_field(desc, "value") }.is_some();
        if value_present || unsafe { desc_field(desc, "writable") }.is_some() {
            if let Some((v, o)) = get_f {
                unsafe { release_desc_field(v, o) };
            }
            if let Some((v, o)) = set_f {
                unsafe { release_desc_field(v, o) };
            }
            let msg = if value_present {
                c"Invalid property.  'value' present on property with getter or setter."
            } else {
                c"Invalid property.  'writable' present on property with getter or setter."
            };
            unsafe { __torajs_throw_type_error(msg.as_ptr() as *const u8) };
            return true;
        }
        // take_accessor_closure answers an OWNED ref per closure
        // (borrows inc, getter products transfer); the pair takes
        // both, and the pair's own +1 transfers into the entry via
        // define_apply's ANY_HEAP consume.
        let Ok(get_ptr) = (unsafe { take_accessor_closure(get_f, "get") }) else {
            if let Some((v, o)) = set_f {
                unsafe { release_desc_field(v, o) };
            }
            return true;
        };
        let Ok(set_ptr) = (unsafe { take_accessor_closure(set_f, "set") }) else {
            if !get_ptr.is_null() {
                unsafe { __torajs_value_drop_heap(get_ptr) };
            }
            return true;
        };
        // RFC 20260717-fnexpr-this-channel knife 5 — a fn-expr face
        // whose body says `this` carries FLAG_CLOSURE_RECV_FIRST on
        // its header; its kinds byte gains ACC_KIND_RECV so the pair
        // invoke puts the receiver in argv[0]. Runtime descriptor
        // walks (defineProperties / Object.create nesting) land here.
        let kinds = (crate::accessor::ACC_KIND_BOXED as u64 | unsafe { recv_kind_bit(get_ptr) })
            | ((crate::accessor::ACC_KIND_BOXED as u64 | unsafe { recv_kind_bit(set_ptr) }) << 8);
        let pair = unsafe { crate::accessor::__torajs_accessor_pair_new(get_ptr, set_ptr, kinds) };
        flags_byte |= DEFINE_PRESENT_VALUE;
        // Per-face present bits (chunk D) — the redefine merge keeps
        // the current face when absent from the descriptor.
        if get_present {
            flags_byte |= crate::layout::DEFINE_PRESENT_GET;
        }
        if set_present {
            flags_byte |= crate::layout::DEFINE_PRESENT_SET;
        }
        if let Some((e, o)) = unsafe { desc_field_get(desc, "enumerable") } {
            flags_byte |= DEFINE_PRESENT_ENUMERABLE;
            if unsafe { __torajs_anyv_to_bool(e) } {
                flags_byte |= DEFINE_FLAG_ENUMERABLE;
            }
            unsafe { release_desc_field(e, o) };
        }
        if let Some((c, o)) = unsafe { desc_field_get(desc, "configurable") } {
            flags_byte |= DEFINE_PRESENT_CONFIGURABLE;
            if unsafe { __torajs_anyv_to_bool(c) } {
                flags_byte |= DEFINE_FLAG_CONFIGURABLE;
            }
            unsafe { release_desc_field(c, o) };
        }
        unsafe { define_apply(obj_slot, key, ANY_HEAP, pair as u64, flags_byte) };
        return true;
    }
    false
}
