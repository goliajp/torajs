//! `Object.getOwnPropertyDescriptor` arm for a `Tag::Closure` cell
//! (RFC 20260711-closure-reflection chunk B).
//!
//! ES §20.2.4: every function object carries own `name` and `length`
//! data properties `{ writable: false, enumerable: false,
//! configurable: true }`. tr carries the pair virtually — the value
//! sides live in torajs-anyvalue's metadata chain
//! (`__torajs_closure_name_str`: method-cell interned name / bound
//! `"bound "` prefix / fn-addr registry; `__torajs_closure_length`:
//! method-cell arity / bound subtract / registry arity, `-1` = miss).
//!
//! Own-property order: a live expando entry (monkey-patch, or a
//! post-delete recreate once chunk C lands the tombstones) wins over
//! the virtual pair — the probe delegates to the DynObj descriptor
//! path so accessor entries and attribute flags come out right.
//! `name` always answers (registry misses answer the ES
//! anonymous-function `""`); `length` answers only when the metadata
//! chain has an arity, mirroring the `.length` member read.

use core::ffi::c_void;

use crate::reflect::{VALUE_UNDEFINED_IMM, build_data_descriptor};

unsafe extern "C" {
    /// torajs-anyvalue — owned name Str (interned immortal for
    /// method cells, fresh rc=1 otherwise; drop no-ops on statics).
    fn __torajs_closure_name_str(ptr: *mut c_void) -> *mut u8;
    /// torajs-anyvalue — metadata-chain arity, `-1` = miss.
    fn __torajs_closure_length(ptr: *mut c_void) -> i64;
    /// torajs-anyvalue — the §20.2.4 lazy `prototype` mint, boxed
    /// (0 when the cell owns none). Writes the entry into the props
    /// dict, so it runs at most once per cell.
    fn __torajs_closure_prototype_any(env: *mut c_void) -> u64;
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    /// torajs-anyvalue — ctor-static probe (RFC 20260720 刀 2): the
    /// interned ns-static value cell when `cell` is a builtin ctor
    /// cell and `key` names one of its table statics, NULL on miss.
    /// Immortal cell — the descriptor's value slot takes it without
    /// a ledger.
    fn __torajs_ctor_static_value_cell(cell: *const c_void, key: *const c_void) -> *mut u8;
    /// torajs-anyvalue — ctor `prototype` probe (RFC 20260721 刀 1):
    /// the builtin `<Ctor>.prototype` singleton, owned +1 (the
    /// descriptor's value slot takes the reference). NULL on miss.
    fn __torajs_ctor_prototype_cell(cell: *const c_void, key: *const c_void) -> *mut u8;
    /// torajs-anyvalue — the §6.1.5.1 well-known symbol a `Symbol`
    /// ctor cell owns under `key` (owned +1; immortal singleton).
    fn __torajs_ctor_wellknown_symbol(cell: *const c_void, key: *const c_void) -> *mut c_void;
    /// torajs-anyvalue — §21.1.2 Number data-constant probe (RFC
    /// 20260721 刀 1): writes the (slot-tag, payload) immediate pair,
    /// answers 1 on hit / 0 on miss.
    fn __torajs_ctor_number_constant(
        cell: *const c_void,
        key: *const c_void,
        out_tag: *mut u64,
        out_val: *mut u64,
    ) -> i64;
}

const ANY_I64: u64 = 2;
const ANY_HEAP: u64 = 4;

/// `torajs_rc::FLAG_FN_NAME_DELETED` / `FLAG_FN_LENGTH_DELETED`
/// mirrors (chunk C tombstones; this crate keeps its dep tree
/// narrow — the u16 bit positions are part of the header ABI).
/// Bits 10-11: 13-14 are the cycle-collector color field (RFC
/// 20260713-defprop-residual-cluster chunk A).
/// `torajs_rc::FLAG_FN_PROTO` mirror (bit 15, Closure-private) — set on
/// a plain `function` cell, clear for arrow / method forms, which own no
/// `prototype` (§20.2.4 vs §10.2.5).
const FLAG_FN_PROTO: u16 = 1 << 15;
const FLAG_FN_NAME_DELETED: u16 = 1 << 10;
const FLAG_FN_LENGTH_DELETED: u16 = 1 << 11;

/// Closure-env layout mirror (torajs-core `ssa_lower.rs` constants):
/// expando props-dynobj slot at +24.
const CLOSURE_PROPS_OFF: usize = 24;

/// `true` iff the key Str spells exactly `name`.
pub(crate) unsafe fn key_is(key: *const c_void, name: &[u8]) -> bool {
    unsafe { crate::str_wtf8::StrWtf8::of(key) }.as_bytes() == name
}

/// See module doc.
///
/// # Safety
/// `cell` is a live `Tag::Closure` heap pointer (caller checked the
/// header tag); `key` is a live `Str` pointer (caller checked
/// non-NULL).
pub(crate) unsafe fn closure_cell_descriptor(cell: *const c_void, key: *const c_void) -> u64 {
    // 1. expando entry wins — delegate to the DynObj descriptor path
    //    (accessor entries / attribute flags handled there).
    let props = unsafe {
        cell.cast::<u8>()
            .add(CLOSURE_PROPS_OFF)
            .cast::<*const c_void>()
            .read()
    };
    if !props.is_null() && unsafe { __torajs_dynobj_has(props, key as *const u8) } {
        return unsafe {
            crate::reflect_get_property_descriptor::__torajs_anyv_get_property_descriptor(
                props as u64,
                key,
            )
        };
    }
    // 2. the virtual §20.2.4 pair — `{ writable: false, enumerable:
    //    false, configurable: true }`. The owned name Str transfers
    //    into the descriptor (no extra inc). A chunk-C tombstone
    //    (delete fn.name / fn.length) skips the virtual answer.
    let flags = unsafe { (cell.cast::<u8>().add(6) as *const u16).read() };
    if unsafe { key_is(key, b"name") } && flags & FLAG_FN_NAME_DELETED == 0 {
        let s = unsafe { __torajs_closure_name_str(cell as *mut c_void) };
        return unsafe { build_data_descriptor(ANY_HEAP, s as u64, 0, 0, 1) };
    }
    if unsafe { key_is(key, b"length") } && flags & FLAG_FN_LENGTH_DELETED == 0 {
        let l = unsafe { __torajs_closure_length(cell as *mut c_void) };
        if l >= 0 {
            return unsafe { build_data_descriptor(ANY_I64, l as u64, 0, 0, 1) };
        }
    }
    // §20.2.4 — a plain `function` owns `prototype` {writable: true,
    // enumerable: false, configurable: false}; an arrow / method form
    // owns none (the FLAG_FN_PROTO header bit is that distinction).
    // The mint is lazy (RFC 20260721 刀 9) and writes into the props
    // dict, so a later call takes step 1's expando answer instead —
    // this arm only runs before the first read. The descriptor takes
    // its own stake on the returned cell.
    if flags & FLAG_FN_PROTO != 0 && unsafe { key_is(key, b"prototype") } {
        let boxed = unsafe { __torajs_closure_prototype_any(cell as *mut c_void) };
        if boxed != 0 {
            let t = unsafe { __torajs_anyv_unbox_tag(boxed) } as u64;
            let v = unsafe { __torajs_anyv_unbox_value(boxed) } as u64;
            if t == ANY_HEAP {
                unsafe { __torajs_rc_inc(v as *mut c_void) };
            }
            return unsafe { build_data_descriptor(t, v, 1, 0, 0) };
        }
    }
    // 3. ctor-static own descriptor (RFC 20260720 刀 2) — a builtin
    //    ctor cell answers its table statics as the ES §19/§20
    //    builtin-static shape `{ writable: true, enumerable: false,
    //    configurable: true }`, value = the interned ns-static cell
    //    (the same identity a value read mints).
    let v = unsafe { __torajs_ctor_static_value_cell(cell, key) };
    if !v.is_null() {
        return unsafe { build_data_descriptor(ANY_HEAP, v as u64, 1, 0, 1) };
    }
    // 4. ctor `prototype` own descriptor (RFC 20260721 刀 1) —
    //    §20.x.2.x on every builtin family: `{ writable: false,
    //    enumerable: false, configurable: false }`, value = the
    //    builtin prototype singleton (owned +1 from the probe).
    let p = unsafe { __torajs_ctor_prototype_cell(cell, key) };
    if !p.is_null() {
        return unsafe { build_data_descriptor(ANY_HEAP, p as u64, 0, 0, 0) };
    }
    // 5. Number data constants (RFC 20260721 刀 1) — §21.1.2 value
    //    properties, all `{ writable: false, enumerable: false,
    //    configurable: false }`; immediates carry no ledger.
    let (mut c_tag, mut c_val) = (0u64, 0u64);
    if unsafe { __torajs_ctor_number_constant(cell, key, &mut c_tag, &mut c_val) } != 0 {
        return unsafe { build_data_descriptor(c_tag, c_val, 0, 0, 0) };
    }
    // 6. Symbol well-known data statics (RFC 20260722 刀 2) —
    //    §6.1.5.1: `{ writable: false, enumerable: false,
    //    configurable: false }`, value = the singleton (owned +1
    //    from the probe, transfers into the descriptor).
    let s = unsafe { __torajs_ctor_wellknown_symbol(cell, key) };
    if !s.is_null() {
        return unsafe { build_data_descriptor(ANY_HEAP, s as u64, 0, 0, 0) };
    }
    VALUE_UNDEFINED_IMM
}
