//! RFC 20260912-global-var-cell — §16.1.7 GlobalDeclarationInstantiation
//! for a script's top-level `var`: the binding becomes an own data
//! property of the global object, and the two are ONE storage.
//!
//! The sibling `globalthis_object_idents` answers what an
//! UNRESOLVABLE name gets from the object; this file is the other
//! direction — a name the program declares, handed to the object so
//! the reflective face (`globalThis.v`, `this.v`, `with (globalThis)`,
//! `Object.getOwnPropertyDescriptor`, `delete this.v`) reads and
//! writes the very slot the compiled bare name loads and stores.
//!
//! The entry's value is a `GlobalCell` over the slot (torajs-dynobj
//! `global_cell`); the dynobj kernels see through it. Attributes per
//! §16.1.7 step 17 / §9.1.1.4.17 CreateGlobalVarBinding: writable and
//! enumerable, NOT configurable — which is what makes `delete this.x`
//! answer false and `verifyProperty` see a var.
//!
//! This is the one caller of the bag that mints on every run of a
//! script with a `var` in it — the bag only (no §19 fill), the same
//! split `globalthis_bag_ptr` documents.

use core::ffi::c_void;
use core::sync::atomic::Ordering;

use super::globalthis_object::{GLOBALTHIS_OBJECT, globalthis_bag_ptr};

unsafe extern "C" {
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_entry_is_global_cell(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_flags(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_global_cell_alloc(
        slot_ptr: *mut c_void,
        kind: u64,
        init_ptr: *const c_void,
    ) -> *mut c_void;
    fn __torajs_dynobj_set_entry_flags(obj_slot: *mut *mut c_void, key: *mut c_void, flags: u64);
    fn __torajs_dynobj_set_entry_value_raw(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_rc_dec(p: *mut c_void) -> i32;
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_reference_error_name(name: *mut c_void);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
}

/// `torajs_dynobj::layout` mirrors — the header flag a
/// `preventExtensions` sets, the three get_flags bits, and the
/// define descriptor bits.
const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;
const GET_FLAG_WRITABLE: u64 = 1 << 0;
const GET_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
/// `AnySlotTag::Heap` / `AnySlotTag::Undef` / the get_tag accessor
/// sentinel.
const ANY_HEAP: u64 = 4;
const ANY_UNDEF: u64 = 5;
const ANY_ACCESSOR: u64 = 6;
/// `torajs_dynobj::global_cell::KIND_ANY`.
const KIND_ANY: u64 = 0;
/// `torajs_dynobj::global_cell::KIND_FLAG_WRITERLESS` — bit 8 of the
/// cell's kind word; bit 9 of this kernel's `kind` argument.
const KIND_FLAG_WRITERLESS: u64 = 1 << 8;

/// `__torajs_global_var_bind(key, slot_ptr, kind)` — §9.1.1.4.17
/// CreateGlobalVarBinding, for the slot the compiled program keeps
/// the binding in. `kind` carries the cell kind in its low byte; bit
/// 8 says whether the property is configurable — a written `var` is
/// not, a synthesized implicit global (the property a [[Set]] would
/// have created, §9.1.1.4.6) and an eval's `var` (§19.2.1.3) are; bit
/// 9 says the binding has no bare compiled writer (see
/// `global_cell::KIND_FLAG_WRITERLESS`); bit 11 says the slot already
/// holds the value being written (an assignment site, not the `x = x`
/// an init-less eval `var` hoists to); bit 12 says the binding is an
/// implicit global — a [[Set]]'s creation, which a non-extensible
/// object refuses silently rather than with the `var` TypeError.
///
/// - Absent: define `{[[Value]]: <slot>, W: true, E: true, C: <bit 8>}`
///   — a `GlobalCell` over the slot — unless the object is not
///   extensible (§16.1.7 step 12.b: TypeError).
/// - Present as a data property (`this.v = 1` ran before `var v`, or
///   `Object.defineProperty(this, "v", …)` did): the property is kept
///   and the binding takes its value (§16.1.7 CanDeclareGlobalVar says
///   "do nothing", which for one storage means the slot now holds
///   what the object held) — UNLESS bit 11 says the slot already
///   holds the value being written (a real `x = e`: §9.1.1.4.6
///   [[Set]] on the existing property), in which case the property
///   keeps its attributes and the slot's value is the binding's.
///   Only an `any` slot can take an arbitrary
///   value; a typed slot refuses loudly. A READ-ONLY property is kept
///   only over a writerless binding (bit 9) — the compiled bare writes
///   could not honour it otherwise — and its value lands through the
///   raw entry swap, since `set` rightly refuses a read-only entry.
/// - Present as a cell already: nothing to do (the same binding,
///   declared twice).
/// - Present as an accessor, or read-only over a binding that is
///   written: refused loudly.
///
/// # Safety
///
/// `key` is a live Str cell (borrowed); `slot_ptr` is the address of
/// a live data global that outlives the program.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_var_bind(
    key: *mut c_void,
    slot_ptr: *mut c_void,
    kind: u64,
) {
    let configurable = kind & (1 << 8) != 0;
    let writerless = kind & (1 << 9) != 0;
    let function = kind & (1 << 10) != 0;
    let assigned = kind & (1 << 11) != 0;
    let implicit = kind & (1 << 12) != 0;
    let kind = kind & 0xff;
    let cell_kind = kind | if writerless { KIND_FLAG_WRITERLESS } else { 0 };
    let mut obj = globalthis_bag_ptr();
    // SAFETY: `obj` is the minted singleton; `key` a Str cell per the
    // caller invariant; every callee borrows both. The cell minted
    // below is fresh (rc 1) and its single reference transfers into
    // the entry through `define_plain` / `set`.
    unsafe {
        if __torajs_dynobj_has(obj, key) != 0 {
            // The RAW entry, not `get_tag` / `get_value`: those see
            // through a cell to the binding's value, so asking them
            // "is it a cell" answered no on every second write and
            // minted a cell over a cell — the slot then held the new
            // cell's pointer as its value (rotation 636, `x += "s"`
            // on an implicit global printed `[object]`).
            if __torajs_dynobj_entry_is_global_cell(obj, key) != 0 {
                return;
            }
            let cur_tag = __torajs_dynobj_get_tag(obj, key);
            let cur_value = __torajs_dynobj_get_value(obj, key);
            let cur_flags = __torajs_dynobj_get_flags(obj, key);
            let writable = cur_flags & GET_FLAG_WRITABLE != 0;
            if function {
                // §9.1.1.4.18 steps 5-6: the function value wins (the
                // slot already holds it). A configurable property is
                // redefined to the function attributes; a non-
                // configurable writable data property keeps its
                // attributes and takes the value; anything else cannot
                // take it — DefinePropertyOrThrow's TypeError.
                let cur_configurable = cur_flags & GET_FLAG_CONFIGURABLE != 0;
                if !cur_configurable && (cur_tag == ANY_ACCESSOR || !writable) {
                    __torajs_throw_type_error(
                        c"Cannot declare a global function over a non-configurable property that is an accessor or read-only."
                            .as_ptr(),
                    );
                    return;
                }
                let cell = __torajs_global_cell_alloc(slot_ptr, kind, core::ptr::null());
                if cur_configurable {
                    // The flags upsert resets the value slot (it revives
                    // holes); the cell lands after it.
                    __torajs_dynobj_set_entry_flags(
                        &mut obj,
                        key,
                        DEFINE_FLAG_WRITABLE
                            | DEFINE_FLAG_ENUMERABLE
                            | if configurable {
                                DEFINE_FLAG_CONFIGURABLE
                            } else {
                                0
                            },
                    );
                }
                __torajs_dynobj_set_entry_value_raw(&mut obj, key, ANY_HEAP, cell as u64);
                GLOBALTHIS_OBJECT.store(obj as u64, Ordering::Release);
                return;
            }
            if cur_tag == ANY_ACCESSOR || (!writable && !writerless) || kind != KIND_ANY {
                __torajs_throw_type_error(
                    c"A `var` declaration over an existing global property that is an accessor, read-only, or typed is not yet supported."
                        .as_ptr(),
                );
                return;
            }
            // The slot takes its own stake on a heap payload; the
            // entry keeps its own until the cell swaps in and drops it.
            // A store the slot already holds (bit 11) IS the value —
            // copying the property's back over it is what read `0`
            // after `q = 3` (rotation 639).
            if !assigned {
                if cur_tag == ANY_HEAP {
                    __torajs_rc_inc(cur_value as *mut c_void);
                }
                *(slot_ptr as *mut u64) =
                    __torajs_anyv_box_from_pair(cur_tag as i64, cur_value as i64);
            }
            let cell = __torajs_global_cell_alloc(slot_ptr, cell_kind, core::ptr::null());
            if writable {
                __torajs_dynobj_set(&mut obj, key, ANY_HEAP, cell as u64);
            } else {
                __torajs_dynobj_set_entry_value_raw(&mut obj, key, ANY_HEAP, cell as u64);
            }
        } else {
            let header_flags = *(obj.cast::<u8>().add(6) as *const u16);
            if header_flags & HDR_FLAG_NON_EXTENSIBLE != 0 {
                if implicit && kind == KIND_ANY {
                    // §9.1.1.4.6 SetMutableBinding → §10.1.9.2 step 2.c:
                    // CreateDataProperty on a non-extensible object is
                    // false, and sloppy code (the only code that reaches
                    // an implicit write) discards it: NO binding. The
                    // slot the store just filled goes back to undefined,
                    // so `typeof zz` keeps answering "undefined".
                    let stored = *(slot_ptr as *const u64);
                    if __torajs_anyv_unbox_tag(stored) as u64 == ANY_HEAP {
                        __torajs_rc_dec(__torajs_anyv_unbox_value(stored) as *mut c_void);
                    }
                    *(slot_ptr as *mut u64) = __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0);
                    return;
                }
                __torajs_throw_type_error(
                    c"Cannot declare a global variable: the global object is not extensible."
                        .as_ptr(),
                );
                return;
            }
            let cell = __torajs_global_cell_alloc(slot_ptr, cell_kind, core::ptr::null());
            let flags = DEFINE_PRESENT_VALUE
                | DEFINE_PRESENT_WRITABLE
                | DEFINE_FLAG_WRITABLE
                | DEFINE_PRESENT_ENUMERABLE
                | DEFINE_FLAG_ENUMERABLE
                | DEFINE_PRESENT_CONFIGURABLE
                | if configurable {
                    DEFINE_FLAG_CONFIGURABLE
                } else {
                    0
                };
            __torajs_dynobj_define_plain(&mut obj, key, ANY_HEAP, cell as u64, flags);
        }
    }
    GLOBALTHIS_OBJECT.store(obj as u64, Ordering::Release);
}

/// `__torajs_global_var_sync(key, slot_ptr)` — §9.1.1.4 GetBindingValue
/// for a name that only a LATER eval / Script declares (`EvalVar` /
/// `ScriptVar`: the property binds where the text runs, see
/// `ssa_lower_ident_site_bound`). Until then the compiled slot is not
/// the binding — the name resolves on the object half of the record —
/// so a read refreshes the slot from the object's own property first:
/// an accessor's getter runs (§7.3.2, `this` = the global object, and
/// its throw is recorded for the caller's check); a data property's
/// value is copied, with a stake of its own on a heap payload. Once
/// the text has bound the property, the entry is a cell over this slot
/// and nothing moves. An absent property is an unbound name — every
/// compiled write of the name binds it, so the text has not run, or
/// the binding was deleted — and the read a ReferenceError, recorded
/// for the caller's check.
///
/// The slot owns what it holds, so the payload it held is released.
///
/// # Safety
///
/// `key` is a live Str cell (borrowed); `slot_ptr` is the address of a
/// live `any` data global that outlives the program.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_global_var_sync(key: *mut c_void, slot_ptr: *mut c_void) {
    let obj = GLOBALTHIS_OBJECT.load(Ordering::Acquire) as *mut c_void;
    // SAFETY: `obj` is the singleton or null (checked); `key` a Str
    // cell and `slot_ptr` a live slot per the caller invariant; every
    // callee borrows what it is handed.
    unsafe {
        let fresh = if obj.is_null() || __torajs_dynobj_has(obj, key) == 0 {
            __torajs_throw_reference_error_name(key);
            __torajs_anyv_box_from_pair(ANY_UNDEF as i64, 0)
        } else if __torajs_dynobj_entry_is_global_cell(obj, key) != 0 {
            return;
        } else {
            let tag = __torajs_dynobj_get_tag(obj, key);
            let value = __torajs_dynobj_get_value(obj, key);
            if tag == ANY_ACCESSOR {
                __torajs_accessor_invoke_getter(
                    value as *const c_void,
                    crate::nanbox::box_void_ptr(obj),
                )
            } else {
                if tag == ANY_HEAP {
                    __torajs_rc_inc(value as *mut c_void);
                }
                __torajs_anyv_box_from_pair(tag as i64, value as i64)
            }
        };
        let old = *(slot_ptr as *const u64);
        *(slot_ptr as *mut u64) = fresh;
        if __torajs_anyv_unbox_tag(old) as u64 == ANY_HEAP {
            let p = __torajs_anyv_unbox_value(old) as *mut c_void;
            if __torajs_rc_dec(p) == 1 {
                __torajs_value_drop_heap(p);
            }
        }
    }
}
