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
    fn __torajs_global_cell_alloc(slot_ptr: *mut c_void, kind: u64) -> *mut c_void;
    fn __torajs_dynobj_set_entry_value_raw(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
}

/// `torajs_dynobj::layout` mirrors — the header flag a
/// `preventExtensions` sets, the three get_flags bits, and the
/// define descriptor bits.
const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;
const GET_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_WRITABLE: u64 = 1 << 0;
const DEFINE_FLAG_ENUMERABLE: u64 = 1 << 1;
const DEFINE_FLAG_CONFIGURABLE: u64 = 1 << 2;
const DEFINE_PRESENT_WRITABLE: u64 = 1 << 3;
const DEFINE_PRESENT_ENUMERABLE: u64 = 1 << 4;
const DEFINE_PRESENT_CONFIGURABLE: u64 = 1 << 5;
const DEFINE_PRESENT_VALUE: u64 = 1 << 6;
/// `AnySlotTag::Heap` / the get_tag accessor sentinel.
const ANY_HEAP: u64 = 4;
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
/// `global_cell::KIND_FLAG_WRITERLESS`).
///
/// - Absent: define `{[[Value]]: <slot>, W: true, E: true, C: <bit 8>}`
///   — a `GlobalCell` over the slot — unless the object is not
///   extensible (§16.1.7 step 12.b: TypeError).
/// - Present as a data property (`this.v = 1` ran before `var v`, or
///   `Object.defineProperty(this, "v", …)` did): the property is kept
///   and the binding takes its value (§16.1.7 CanDeclareGlobalVar says
///   "do nothing", which for one storage means the slot now holds
///   what the object held). Only an `any` slot can take an arbitrary
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
            let writable = __torajs_dynobj_get_flags(obj, key) & GET_FLAG_WRITABLE != 0;
            if cur_tag == ANY_ACCESSOR || (!writable && !writerless) || kind != KIND_ANY {
                __torajs_throw_type_error(
                    c"A `var` declaration over an existing global property that is an accessor, read-only, or typed is not yet supported."
                        .as_ptr(),
                );
                return;
            }
            // The slot takes its own stake on a heap payload; the
            // entry keeps its own until the cell swaps in and drops it.
            if cur_tag == ANY_HEAP {
                __torajs_rc_inc(cur_value as *mut c_void);
            }
            *(slot_ptr as *mut u64) = __torajs_anyv_box_from_pair(cur_tag as i64, cur_value as i64);
            let cell = __torajs_global_cell_alloc(slot_ptr, cell_kind);
            if writable {
                __torajs_dynobj_set(&mut obj, key, ANY_HEAP, cell as u64);
            } else {
                __torajs_dynobj_set_entry_value_raw(&mut obj, key, ANY_HEAP, cell as u64);
            }
        } else {
            let header_flags = *(obj.cast::<u8>().add(6) as *const u16);
            if header_flags & HDR_FLAG_NON_EXTENSIBLE != 0 {
                __torajs_throw_type_error(
                    c"Cannot declare a global variable: the global object is not extensible."
                        .as_ptr(),
                );
                return;
            }
            let cell = __torajs_global_cell_alloc(slot_ptr, cell_kind);
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
