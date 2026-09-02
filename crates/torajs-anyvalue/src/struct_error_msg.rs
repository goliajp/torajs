//! RFC 20260718-error-message-own-prop 刀 1 — `FLAG_ERROR` struct
//! cell `message` own-property semantics + the struct data-field
//! write arm.
//!
//! tr models an error instance's `message` as an always-present
//! class-layout Str field; ES §20.5.6.1.1 makes it an ordinary
//! `{ [[Writable]]: true, [[Enumerable]]: false, [[Configurable]]:
//! true }` data property that only exists when the constructor got a
//! non-undefined message. Own-ABSENCE is carried by the immortal
//! `undefined` sentinel Str cell in the slot (RFC 20260707 — every
//! reflection reader already normalizes the sentinel to JS
//! `undefined`), so:
//!
//! - `delete err.message` detaches by swapping the sentinel in
//!   (drops the old Str) and answers true — the [[Configurable]]
//!   face `prop_delete`'s struct arm previously refused wholesale.
//! - `hasOwnProperty` / gOPD answer absent when the slot holds the
//!   sentinel.
//! - `propertyIsEnumerable` answers false even when present.
//!
//! The data-field write arm ([`struct_data_field_set`]) is the
//! [[Writable]] face: `any`-receiver assignment into a class-layout
//! DATA field, gated on slot-type compatibility (Any / Str / I64 /
//! F64 / Bool). A mismatched payload falls back to the caller's loud
//! reject — a typed slot never silently coerces (shape transition is
//! a recorded follow-up, `.claude/rfcs/20260718-error-message-own-prop`).

use core::ffi::c_void;

use torajs_rc::{AnySlotTag, Tag};

unsafe extern "C" {
    // torajs-structmeta — class-layout read side (struct_probe twins).
    fn __torajs_struct_layout_lookup(class_tag: u32) -> *const c_void;
    fn __torajs_struct_field_find(layout: *const c_void, name: *const u8, name_len: u32) -> u32;
    fn __torajs_struct_field_info(layout: *const c_void, idx: u32) -> FieldInfo;
    // torajs-str — the immortal `undefined` sentinel cell: address
    // mint + identity probe (RFC 20260707 chunk 2/3).
    fn __torajs_str_undef() -> *mut u8;
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    fn __torajs_str_drop(s: *mut c_void);
    // torajs-rc — frozen header bit (ES §7.3.14: frozen ⇒ every data
    // property [[Writable]] = false).
    fn __torajs_obj_is_frozen(p: *const c_void) -> bool;
    // torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

use crate::nanbox_ffi::__torajs_anyv_rc_dec;

/// Mirror of `torajs-structmeta::FieldInfo` (`struct_probe.rs` twin).
#[repr(C)]
struct FieldInfo {
    field_byte_offset: u32,
    type_tag: u8,
}

/// Layout mirrors (`struct_probe.rs` twins).
const OBJ_CLASS_TAG_OFF: usize = 8;

/// The two injected-layout slots that carry own-ABSENCE through the
/// sentinel rather than through the field list. `message` is absent
/// when the ctor got none (§20.5.1.1) or after a delete; `name` is
/// absent on every construction, because §20.5.3.2 puts it on
/// `Error.prototype` and only user code assigning `this.name` makes
/// an instance own one.
const ABSENCE_SLOTS: [&[u8]; 2] = [b"message", b"name"];

/// Resolve a named field slot of a `FLAG_ERROR` struct cell. `None`
/// when the cell is not an error instance or its layout has no such
/// field (a user subclass that shadowed the walk — desugar
/// field-flattening makes that unreachable today, but the probe stays
/// total).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
unsafe fn error_slot(ptr: *const c_void, field: &[u8]) -> Option<*mut u64> {
    if !unsafe { crate::member_get::header_flag(ptr, torajs_rc::FLAG_ERROR) } {
        return None;
    }
    unsafe { layout_slot(ptr, field) }
}

/// [`error_slot`] without the error gate — the layout lookup alone.
/// The typed `.message` / `.name` emit fires on layout SHAPE (an
/// error's field trio), and structurally identical classes share one
/// StructId, so the helper it calls can receive a cell that is not an
/// error at all. Such a cell has no class-prototype story: it answers
/// straight out of its slot.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
unsafe fn layout_slot(ptr: *const c_void, field: &[u8]) -> Option<*mut u64> {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return None;
    }
    let idx = unsafe { __torajs_struct_field_find(layout, field.as_ptr(), field.len() as u32) };
    if idx == u32::MAX {
        return None;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    Some(unsafe { ptr.cast::<u8>().add(info.field_byte_offset as usize) } as *mut u64)
}

/// Whether `ptr` is a `FLAG_ERROR` cell whose `field` slot holds the
/// own-absence sentinel.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
unsafe fn error_field_is_absent(ptr: *const c_void, field: &[u8]) -> bool {
    match unsafe { error_slot(ptr, field) } {
        Some(slot) => {
            let raw = unsafe { slot.read() };
            raw != 0 && unsafe { __torajs_str_is_undef(raw as *const u8) } != 0
        }
        None => false,
    }
}

/// Whether `ptr` is a `FLAG_ERROR` cell whose `message` slot holds
/// the own-absence sentinel.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn error_message_is_absent(ptr: *const c_void) -> bool {
    unsafe { error_field_is_absent(ptr, b"message") }
}

/// Whether `ptr` is a `FLAG_ERROR` cell whose `name` slot holds the
/// own-absence sentinel — i.e. nobody has assigned `this.name`, so
/// the property is the prototype's and not the instance's.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn error_name_is_absent(ptr: *const c_void) -> bool {
    unsafe { error_field_is_absent(ptr, b"name") }
}

/// `delete err.message` — §20.5.6.1.1 msgDesc [[Configurable]]:
/// true. Swaps the own-absence sentinel into the slot (dropping the
/// old Str; a re-delete is an idempotent spec success). Answers 1
/// on the error-message slot, 0 for anything else (the caller keeps
/// the fixed-layout refusal for ordinary struct fields).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn error_message_delete(ptr: *mut c_void, key: *const c_void) -> i64 {
    if !unsafe { crate::prop_has::key_is(key, b"message") } {
        return 0;
    }
    let Some(slot) = (unsafe { error_slot(ptr, b"message") }) else {
        return 0;
    };
    let old = unsafe { slot.read() };
    if old != 0 && unsafe { __torajs_str_is_undef(old as *const u8) } == 0 {
        unsafe { __torajs_str_drop(old as *mut c_void) };
    }
    unsafe { slot.write(__torajs_str_undef() as u64) };
    1
}

/// Whether `key` names one of the [`ABSENCE_SLOTS`] in its ABSENT
/// state (`FLAG_ERROR` cell + the sentinel in that slot) — the
/// hasOwnProperty / gOPD / member-get miss gate. The caller answers
/// the miss by walking the prototype chain with the same key.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn error_absent_key(ptr: *const c_void, key: *const c_void) -> bool {
    ABSENCE_SLOTS.iter().any(|field| unsafe {
        crate::prop_has::key_is(key, field) && error_field_is_absent(ptr, field)
    })
}

/// C ABI own-presence probe — the typed tier's
/// `err.hasOwnProperty("message")` emit (a compile-time fold can't
/// see the runtime absent state).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_message_present(obj: *const c_void) -> i64 {
    (!unsafe { error_message_is_absent(obj) }) as i64
}

/// C ABI own-presence probe for `name` — the typed tier's
/// `err.hasOwnProperty("name")` / `propertyIsEnumerable("name")`
/// emit. Unlike `message`, an own `name` only ever comes from user
/// code assigning it, and such an assignment is an ordinary
/// CreateDataProperty — so own here also means ENUMERABLE, and the
/// two probes share this one answer.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_name_present(obj: *const c_void) -> i64 {
    (!unsafe { error_name_is_absent(obj) }) as i64
}

unsafe extern "C" {
    // torajs-meta classmeta — per-class-tag `__proto_<C>` lookup
    // (OWNED AnyValue: a cell payload arrives +1).
    fn __torajs_anyv_proto_get(tag: i64) -> u64;
    // torajs-dynobj — prototype-object entry probes (borrow reads).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    // torajs-str — key mint for the walk.
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
}

/// The generalized walk behind [`error_message_proto_pair`] — any
/// literal key through the error instance's class prototype chain
/// (rotation 141: the `toString` monkey-patch probe shares it).
/// Same borrow-shaped `(tag, value)` contract; `(ANY_UNDEF, 0)` on
/// a fully missing chain — indistinguishable from a chain entry
/// that literally stores `undefined` (both read as absent).
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer.
pub(crate) unsafe fn error_proto_chain_pair(ptr: *const c_void, key_lit: &[u8]) -> (u64, u64) {
    let key =
        unsafe { __torajs_str_alloc(key_lit.as_ptr(), key_lit.len() as i64) } as *const c_void;
    let out = unsafe { struct_proto_chain_pair(ptr, key) };
    unsafe { __torajs_str_drop(key as *mut c_void) };
    out
}

/// Key-cell twin of [`error_proto_chain_pair`] — the member-get
/// struct-miss arm walks the class prototype chain with the caller's
/// live key (L3b ⑧: an instance `ca.m` / `ca.constructor` read
/// resolves the prototype's own entry — the reified method face, the
/// wired `constructor` — instead of answering undefined). Same
/// borrow-shaped `(tag, value)` contract; `(ANY_UNDEF, 0)` on a
/// fully missing chain.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str cell.
pub(crate) unsafe fn struct_proto_chain_pair(ptr: *const c_void, key: *const c_void) -> (u64, u64) {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let root = unsafe { __torajs_anyv_proto_get(class_tag as i64) };
    // Non-cell (null / unregistered) → no chain. Cell test mirrors
    // `nanbox::is_cell` consumers: top16 clear + low type bit clear.
    if root & 0xFFFF_0000_0000_0000 != 0 || root & 0x2 != 0 || root == 0 {
        return (AnySlotTag::Undef as u64, 0);
    }
    let mut cur = root as *const c_void;
    let mut out = (AnySlotTag::Undef as u64, 0);
    loop {
        if unsafe { __torajs_dynobj_has(cur, key) } != 0 {
            out = (unsafe { __torajs_dynobj_get_tag(cur, key) }, unsafe {
                __torajs_dynobj_get_value(cur, key)
            });
            break;
        }
        match unsafe { crate::member_get_own::user_proto_cell(cur) } {
            Some(next) => cur = next as *const c_void,
            None => break,
        }
    }
    // Release proto_get's +1 — the pair stays a borrow of the value
    // the (still-registered, immortal-for-program-life) prototype
    // object owns.
    unsafe {
        __torajs_anyv_rc_dec(crate::nanbox_encode::__torajs_anyv_box_from_pair(
            AnySlotTag::Heap as i64,
            root as i64,
        ))
    };
    out
}

/// Typed-tier `err.message` read — BORROWED Str pointer, mirroring
/// the struct-field `Load` this emit replaces (the struct / the
/// prototype object keeps the stake; consumers take their own share
/// as usual). Own-present answers the slot; own-absent walks the
/// prototype chain; a chain miss or a non-Str chain value answers
/// the undefined sentinel (a non-Str prototype `message` through the
/// typed Str surface is a recorded boundary — the any tier reads it
/// exactly).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_message_get(obj: *const c_void) -> *mut u8 {
    unsafe { error_slot_get(obj, b"message") }
}

/// Typed-tier `err.name` read — the [`__torajs_error_message_get`]
/// twin, and the reason every `name` reader keeps working after the
/// ctor stopped writing the class name into the slot: own-absent
/// (the ordinary case) walks to `<C>.prototype.name`, which
/// `__torajs_error_proto_install` filled with the spec value.
///
/// Also the resolver behind the three stderr / toString reporters
/// that used to read the `name` field at its fixed offset — they call
/// it rather than repeat the sentinel test three times.
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_error_name_get(obj: *const c_void) -> *mut u8 {
    unsafe { error_slot_get(obj, b"name") }
}

/// Shared body of the two readers above — BORROWED Str pointer,
/// mirroring the struct-field `Load` the typed emit replaces (the
/// struct / the prototype object keeps the stake; consumers take
/// their own share as usual). Own-present answers the slot;
/// own-absent walks the prototype chain; a chain miss or a non-Str
/// chain value answers the undefined sentinel (a non-Str prototype
/// entry read through the typed Str surface is a recorded boundary —
/// the any tier reads it exactly).
///
/// # Safety
/// `obj` is a live `Tag::Obj` heap pointer.
unsafe fn error_slot_get(obj: *const c_void, field: &[u8]) -> *mut u8 {
    if let Some(slot) = unsafe { layout_slot(obj, field) } {
        let raw = unsafe { slot.read() };
        if raw != 0 && unsafe { __torajs_str_is_undef(raw as *const u8) } == 0 {
            return raw as *mut u8;
        }
        // Non-error cell sharing the layout: no chain to consult, so
        // the slot's own answer (NULL = JS null, or the sentinel) is
        // exactly what the field `Load` this replaces would give.
        if !unsafe { crate::member_get::header_flag(obj, torajs_rc::FLAG_ERROR) } {
            return raw as *mut u8;
        }
    }
    let (tag, val) = unsafe { error_proto_chain_pair(obj, field) };
    if tag == AnySlotTag::Heap as u64 && val != 0 {
        let cell_tag = unsafe { (val as *const u8).add(4).cast::<u16>().read() };
        if cell_tag == Tag::Str as u16 {
            return val as *mut u8;
        }
    }
    unsafe { __torajs_str_undef() }
}

/// `any`-receiver assignment into a class-layout DATA field — the
/// §10.1.9 [[Set]] face `member_set`'s struct arm previously
/// rejected wholesale. Consumes the `(tag, value)` payload on
/// success (true); an absent field / accessor spelling / mismatched
/// payload type answers false with the payload untouched so the
/// caller's reject stays loud.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` heap pointer; `key` is a live Str
/// cell; the payload follows the lowering's consume convention
/// (heap tag carries a transferred +1).
pub(crate) unsafe fn struct_data_field_set(
    ptr: *mut c_void,
    key: *const c_void,
    tag: u64,
    value: u64,
) -> bool {
    let class_tag = unsafe { ptr.cast::<u8>().add(OBJ_CLASS_TAG_OFF).cast::<u32>().read() };
    let layout = unsafe { __torajs_struct_layout_lookup(class_tag) };
    if layout.is_null() {
        return false;
    }
    let k = unsafe { torajs_rc::str_wtf8::StrWtf8::of(key) };
    let idx = unsafe { __torajs_struct_field_find(layout, k.as_ptr(), k.len()) };
    if idx == u32::MAX {
        return false;
    }
    let info = unsafe { __torajs_struct_field_info(layout, idx) };
    let slot = unsafe { ptr.cast::<u8>().add(info.field_byte_offset as usize) } as *mut u64;
    // ES §7.3.14 — a frozen struct's data properties are all
    // non-writable; module code is strict so the refused [[Set]]
    // throws. Checked only after the field resolves: a miss stays
    // the caller's (different) reject.
    if unsafe { __torajs_obj_is_frozen(ptr) } {
        unsafe {
            if tag == AnySlotTag::Heap as u64 {
                __torajs_anyv_rc_dec(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                    tag as i64,
                    value as i64,
                ));
            }
            __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
        }
        return true;
    }
    match info.type_tag {
        // Any slot — the NaN-box stores verbatim; the old box's
        // stake is released, the payload's transferred +1 becomes
        // the slot's.
        0 => unsafe {
            let old = slot.read();
            slot.write(crate::nanbox_encode::__torajs_anyv_box_from_pair(
                tag as i64,
                value as i64,
            ));
            __torajs_anyv_rc_dec(old);
            true
        },
        // Str slot — accepts a heap Str cell (the sentinel included:
        // it IS a Str cell whose rc traffic no-ops). Any other heap
        // shape / a non-heap payload falls back to the loud reject.
        4 => unsafe {
            if tag != AnySlotTag::Heap as u64 || value == 0 {
                return false;
            }
            let cell_tag = (value as *const u8).add(4).cast::<u16>().read();
            if cell_tag != Tag::Str as u16 {
                return false;
            }
            let old = slot.read();
            slot.write(value);
            if old != 0 {
                __torajs_str_drop(old as *mut c_void);
            }
            true
        },
        // I64 slot ← I64 payload.
        1 if tag == AnySlotTag::I64 as u64 => unsafe {
            slot.write(value);
            true
        },
        // I64 slot ← an integer-valued F64 payload, converted — the
        // mirror of the F64 arm's I64 conversion below. The typed
        // lanes box a number[] element as F64 whatever its value, so
        // a callback's `this.count = v` met this slot with F64 bits
        // for a plain integer. A fractional value keeps the loud
        // reject: it cannot live in this slot without breaking the
        // layout's invariant.
        1 if tag == AnySlotTag::F64 as u64 => {
            let f = f64::from_bits(value);
            if f.fract() != 0.0 || f < i64::MIN as f64 || f >= i64::MAX as f64 {
                return false;
            }
            unsafe {
                slot.write(f as i64 as u64);
            }
            true
        }
        // F64 slot ← F64 bits, or an I64 payload converted.
        2 if tag == AnySlotTag::F64 as u64 => unsafe {
            slot.write(value);
            true
        },
        2 if tag == AnySlotTag::I64 as u64 => unsafe {
            slot.write((value as i64 as f64).to_bits());
            true
        },
        // Bool slot ← Bool payload.
        3 if tag == AnySlotTag::Bool as u64 => unsafe {
            slot.write(value);
            true
        },
        // Mismatched payload for a typed slot — loud reject upstream
        // (recorded follow-up: shape transition).
        _ => false,
    }
}
