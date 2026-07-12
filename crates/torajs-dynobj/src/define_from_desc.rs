//! `__torajs_dynobj_define_from_desc(obj_slot, key, desc)` — the
//! runtime-descriptor path for `Object.defineProperty`, split out of
//! `define.rs` (file-size hard limit; RFC 20260712-arr-exotic-define
//! chunk B pushed the shared file over 500). Reads the descriptor
//! fields off the `desc` dynobj at runtime (§6.2.6.5
//! ToPropertyDescriptor, accessor pairs included) and applies via
//! [`crate::define::define_apply`] — which also carries the Arr
//! receiver dispatch, so this path inherits it.

use core::ffi::c_void;

use crate::define::define_apply;
use crate::layout::{
    ANY_HEAP, ANY_UNDEF, DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE,
};
use crate::probe::{entries, probe};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const u8);
    fn __torajs_value_drop_heap(child: *mut c_void);
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_anyv_to_bool(v: u64) -> bool;
}

/// Stack-allocated Str-shaped probe key. [`probe`] / `hash_str` /
/// `str_eq` only read `len` (offset 8) and the inline payload (offset
/// 16) — never the heap header — so a non-heap buffer with those two
/// fields suffices to look a property name up in a dynobj without
/// allocating (or interning) a real Str. Field names are short; a
/// 16-byte inline payload covers every descriptor key.
#[repr(C, align(8))]
struct FakeStrKey {
    _header: u64,
    len: u64,
    data: [u8; 16],
}

impl FakeStrKey {
    #[inline]
    fn new(name: &str) -> FakeStrKey {
        let mut k = FakeStrKey {
            _header: 0,
            len: name.len() as u64,
            data: [0u8; 16],
        };
        k.data[..name.len()].copy_from_slice(name.as_bytes());
        k
    }
}

/// Look a property name up in `desc` and return its NaN-box
/// `value_anyv` if present (the property's stored AnyValue).
///
/// # Safety
/// `desc` points at a live dynobj heap block.
#[inline]
unsafe fn desc_field(desc: *const c_void, name: &str) -> Option<u64> {
    let probe_key = FakeStrKey::new(name);
    let pr = unsafe { probe(desc, &probe_key as *const FakeStrKey as *const c_void) };
    if !pr.found {
        return None;
    }
    let ent = unsafe { entries(desc) };
    Some(unsafe { (*ent.add(pr.entry as usize)).value_anyv })
}

/// [`desc_field`] with §6.2.6.5 ToPropertyDescriptor [[Get]]
/// semantics: a descriptor field that is itself an accessor property
/// invokes its getter (test262 defines desc fields via accessors to
/// probe exactly this; a getter-less accessor answers `undefined`).
/// Answers `(anyv, owned)` — `owned` marks a fresh getter product
/// whose ref the caller must consume or drop; a plain data field is
/// a borrow. Caller must check for pending throw when `owned`.
///
/// # Safety
/// Same contract as [`desc_field`].
unsafe fn desc_field_get(desc: *const c_void, name: &str) -> Option<(u64, bool)> {
    let raw = unsafe { desc_field(desc, name) }?;
    if unsafe { crate::accessor::value_is_accessor(raw) } {
        let pair = unsafe { __torajs_anyv_unbox_value(raw) } as *const c_void;
        let v = unsafe { crate::accessor::__torajs_accessor_invoke_getter(pair) };
        return Some((v, true));
    }
    Some((raw, false))
}

/// Consume an `owned` flag from [`desc_field_get`] after the value
/// has been read — drops a fresh heap product's surplus ref
/// (immediates no-op through the cell gate).
unsafe fn release_desc_field(anyv: u64, owned: bool) {
    if owned {
        let tag = unsafe { __torajs_anyv_unbox_tag(anyv) } as u64;
        let val = unsafe { __torajs_anyv_unbox_value(anyv) } as u64;
        if tag == ANY_HEAP && val != 0 {
            unsafe { __torajs_value_drop_heap(val as *mut c_void) };
        }
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
    let tag = unsafe { __torajs_anyv_unbox_tag(anyv) } as u64;
    let val = unsafe { __torajs_anyv_unbox_value(anyv) } as u64;
    if tag == ANY_UNDEF {
        return Ok(core::ptr::null_mut());
    }
    // Closure heap cell — universal header type_tag at +4 (Tag::Closure = 3).
    if tag == ANY_HEAP && val != 0 {
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

/// `__torajs_dynobj_define_from_desc(obj_slot, key, desc)` — the
/// runtime-descriptor path for `Object.defineProperty`. Reads the
/// data-descriptor fields (`value` / `writable` / `enumerable` /
/// `configurable`) off the `desc` dynobj at runtime, builds the
/// `flags_byte` + `(tag, value)` the compile-time literal path
/// produces, and applies via [`define_apply`].
///
/// Accessor descriptors (RFC 20260712 chunk 2): when `get` / `set` is
/// present, an `AccessorPair` is built (mirroring the compile-time
/// `emit_accessor_define` shape — each closure ref is inc'd since the
/// desc keeps its own, the pair's +1 transfers into the entry) and
/// stored as the property value; a descriptor mixing accessor and
/// `value` / `writable` fields throws per §10.1.6.3.
///
/// # Safety
/// `obj_slot` points at a live `*mut c_void` (dynobj or NULL). `key`
/// is a live Str. `desc` is a dynobj heap pointer or NULL. Caller must
/// check for pending throw after return.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_dynobj_define_from_desc(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    desc: *const c_void,
) {
    if desc.is_null() {
        return;
    }

    let mut flags_byte: u64 = 0;
    let mut out_tag: u64 = 0;
    let mut out_value: u64 = 0;

    // §6.2.6.5 ToPropertyDescriptor — every field read goes through
    // [[Get]] (a field that is itself an accessor invokes its
    // getter); presence checks use the raw entry probe.
    let get_f = unsafe { desc_field_get(desc, "get") };
    let set_f = unsafe { desc_field_get(desc, "set") };
    if get_f.is_some() || set_f.is_some() {
        if unsafe { desc_field(desc, "value") }.is_some()
            || unsafe { desc_field(desc, "writable") }.is_some()
        {
            if let Some((v, o)) = get_f {
                unsafe { release_desc_field(v, o) };
            }
            if let Some((v, o)) = set_f {
                unsafe { release_desc_field(v, o) };
            }
            unsafe {
                __torajs_throw_type_error(
                    c"Invalid property descriptor. Cannot both specify accessors and a value or writable attribute."
                        .as_ptr() as *const u8,
                );
            }
            return;
        }
        // take_accessor_closure answers an OWNED ref per closure
        // (borrows inc, getter products transfer); the pair takes
        // both, and the pair's own +1 transfers into the entry via
        // define_apply's ANY_HEAP consume. Runtime closures are
        // any-world (kinds = BOXED/BOXED).
        let Ok(get_ptr) = (unsafe { take_accessor_closure(get_f, "get") }) else {
            if let Some((v, o)) = set_f {
                unsafe { release_desc_field(v, o) };
            }
            return;
        };
        let Ok(set_ptr) = (unsafe { take_accessor_closure(set_f, "set") }) else {
            if !get_ptr.is_null() {
                unsafe { __torajs_value_drop_heap(get_ptr) };
            }
            return;
        };
        let kinds = (crate::accessor::ACC_KIND_BOXED as u64)
            | ((crate::accessor::ACC_KIND_BOXED as u64) << 8);
        let pair = unsafe { crate::accessor::__torajs_accessor_pair_new(get_ptr, set_ptr, kinds) };
        flags_byte |= DEFINE_PRESENT_VALUE;
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
        return;
    }

    if let Some((v_anyv, v_owned)) = unsafe { desc_field_get(desc, "value") } {
        let v_tag = unsafe { __torajs_anyv_unbox_tag(v_anyv) } as u64;
        let v_val = unsafe { __torajs_anyv_unbox_value(v_anyv) } as u64;
        // define_apply consumes one rc of a Heap value; a borrowed
        // field (still owned by `desc`) incs, a getter product
        // transfers its fresh ref as-is.
        if v_tag == ANY_HEAP && v_val != 0 && !v_owned {
            unsafe { __torajs_rc_inc(v_val as *mut c_void) };
        }
        out_tag = v_tag;
        out_value = v_val;
        flags_byte |= DEFINE_PRESENT_VALUE;
    }

    if let Some((w, o)) = unsafe { desc_field_get(desc, "writable") } {
        flags_byte |= DEFINE_PRESENT_WRITABLE;
        if unsafe { __torajs_anyv_to_bool(w) } {
            flags_byte |= DEFINE_FLAG_WRITABLE;
        }
        unsafe { release_desc_field(w, o) };
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

    unsafe { define_apply(obj_slot, key, out_tag, out_value, flags_byte) }
}
