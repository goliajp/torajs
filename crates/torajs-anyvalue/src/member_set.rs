//! `__torajs_any_member_set` — `recv.key = value` where the receiver
//! is an `any` value (the write mirror of the chunk-508 member-read
//! fallback).
//!
//! Pre-tag-dispatch the lowering handed every `any` receiver straight
//! to `__torajs_dynobj_set`, which reads the cell as a DynObj layout —
//! a RegExp / Str / Arr receiver was silent memory corruption (probed:
//! `r.lastIndex = 3` scrambles the regex cell; `s.x = 1` SIGSEGVs).
//! This entry gates on the heap tag first:
//!
//! - `Tag::DynObj` → the ordinary `dynobj_set` path. The receiver
//!   slot is updated in place when the set relocates the block
//!   (realloc-on-grow), re-encoded as a fresh NaN-box cell.
//! - `Tag::RegExp` + the interned `lastIndex` hint → the typed
//!   tier's `regex_set_last_index` kernel (numeric payloads only;
//!   other payload tags raise the boundary TypeError below).
//! - `Tag::Arr` → `arrprops_set` expando (lazily-allocated props
//!   dynobj). `length` writes surface the boundary TypeError — the
//!   truncation semantics are a recorded follow-up, and an expando
//!   shadow would be silent-wrong (reads answer the real length).
//! - everything else (other cells, primitives, null/undefined) →
//!   catchable TypeError, never a blind layout write.
//!
//! Argument ledger: `(tag, value)` arrives with the lowering's +1 on
//! heap payloads (the consume convention `dynobj_set` expects); the
//! non-consuming arms release that reference before throwing.

use core::ffi::c_void;

use torajs_rc::{ANY_RPROP_LAST_INDEX, ANY_WPROP_ARR_LENGTH, FLAG_NON_EXTENSIBLE, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

/// `DYNOBJ_HDR_FLAG_NULL_PROTO` mirror (torajs-dynobj layout).
const DYNOBJ_HDR_FLAG_NULL_PROTO: u16 = 1 << 6;

unsafe extern "C" {
    /// torajs-meta — the Annex B `__proto__` setter core (knife 3).
    fn __torajs_anyv_proto_member_set(obj: u64, proto: u64);
    /// torajs-dynobj — realloc-on-grow set; the slot receives the
    /// possibly-relocated block pointer.
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    /// torajs-arr — expando write through the lazily-allocated
    /// props dynobj.
    fn __torajs_arrprops_set(arr_ptr: *mut c_void, key: *const c_void, tag: i64, value: i64);
    /// torajs-arr — §10.4.2.4 length setter (ToUint32 validate +
    /// real resize) for the dynamic-string-key `o[k] = v` form.
    fn __torajs_arr_set_length_any(arr: *mut c_void, tag: i64, value: i64);
    /// torajs-arr — kind-aware element store (consumes a tag-4 rc).
    fn __torajs_arr_index_set(arr: *mut c_void, idx: i64, tag: u64, value: u64);
    /// torajs-arr — per-index attribute flags (RFC
    /// 20260712-arr-exotic-define chunk C writable gate).
    fn __torajs_arr_index_flags(arr: *const c_void, idx: u64) -> u64;
    /// torajs-arr — the index's AccessorPair, NULL when not an
    /// accessor (RFC 20260713 chunk C).
    fn __torajs_arr_index_accessor(arr: *const c_void, idx: u64) -> *mut c_void;
    /// torajs-dynobj — setter dispatch; `0` = getter-only accessor.
    fn __torajs_accessor_invoke_setter(pair: *const c_void, recv_anyv: u64, value_anyv: u64)
    -> i32;
    /// torajs-arr — re-create a deleted index as a default data
    /// property (hole revive, RFC 20260713 chunk C).
    fn __torajs_arr_index_revive(arr: *mut c_void, key: *mut c_void);
    /// torajs-regex — `re.lastIndex` setter.
    fn __torajs_regex_set_last_index(re: *mut c_void, idx: f64);
    /// torajs-dynobj — fresh empty table for the first closure
    /// expando write.
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-dynobj — probe whether a key already lives in the
    /// entry table (1 = live entry). Used by the wrapper
    /// `[[Extensible]] = false` gate to distinguish new-key writes
    /// (throw) from updates (allowed).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Release the lowering's +1 on a heap payload that no arm consumed.
unsafe fn drop_payload(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(4, value as i64)) };
    }
}

unsafe fn reject(tag: u64, value: u64) {
    unsafe {
        drop_payload(tag, value);
        __torajs_throw_type_error(c"cannot assign to a property of this any value".as_ptr());
    }
}

/// True when `key` names a `Tag::StringWrapper` inherent own
/// property — `"length"` or a canonical code-unit index in
/// `[0, len)` (§10.4.3 StringGetOwnProperty). Both are
/// non-writable: the caller refuses the store instead of letting
/// it shadow through the expando dynobj.
unsafe fn strwrapper_own_domain_key(ptr: *mut c_void, key: *const c_void) -> bool {
    unsafe {
        let k = key as *const u8;
        let key_len = (k.add(STR_LEN_OFF) as *const u32).read();
        let bytes = core::slice::from_raw_parts(k.add(STR_DATA_OFF), key_len as usize);
        if bytes == b"length" {
            return true;
        }
        let Some(idx) = crate::member_get::canonical_index(bytes) else {
            return false;
        };
        let inner = (ptr.cast::<u8>().add(8) as *const *const c_void).read();
        let len = if inner.is_null() {
            0
        } else {
            inner.cast::<u8>().add(STR_LEN_OFF).cast::<u32>().read() as u64
        };
        idx < len
    }
}

/// Wrapper-cell `[[Set]]` rejection when the receiver has
/// `[[Extensible]] = false` and the key is fresh — mirror of
/// `__torajs_dynobj_set`'s new-key gate wording.
unsafe fn reject_non_extensible(tag: u64, value: u64) {
    unsafe {
        drop_payload(tag, value);
        __torajs_throw_type_error(
            c"Attempting to define property on object that is not extensible.".as_ptr(),
        );
    }
}

/// See module doc. `hint` carries the compile-time member-name
/// Str-cell payload offsets — mirror of `struct_probe.rs` (the key is
/// a live Str cell; blade 7 reads its bytes to resolve the accessor).
const STR_LEN_OFF: usize = 8;
const STR_DATA_OFF: usize = 16;

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const MEMBER_SET_CLOSURE_PROPS_OFF: usize = 24;

/// Number/String/Boolean-wrapper lazy props slot — mirror of
/// `torajs-wrapper::WRAPPER_PROPS_OFF` (RFC 20260716 刀 5, rotation
/// 121). Every wrapper cell layout is `[header:8][value:8][props:8]`.
const MEMBER_SET_WRAPPER_PROPS_OFF: usize = 16;

/// intern: `ANY_RPROP_LAST_INDEX` for `lastIndex`,
/// `ANY_WPROP_ARR_LENGTH` for `length`, −1 otherwise.
///
/// # Safety
/// `recv_slot` points at a live AnyValue slot the caller owns; cell
/// receivers are valid heap pointers; `key` is a live Str cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
) {
    unsafe {
        let recv = *recv_slot;
        if !is_cell(recv) {
            reject(tag, value);
            return;
        }
        let ptr = as_void_ptr(recv);
        let cell_tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if cell_tag == Tag::DynObj as u16 {
            // Annex B §B.2.2.1 — `o.__proto__ = v` runs the
            // inherited setter, not an ordinary entry write (RFC
            // 20260717-user-proto-chain knife 3): [[SetPrototypeOf]]
            // with the cycle walk; an invalid v is silently ignored.
            // The null-proto shape has no inherited setter — its
            // write IS an ordinary own entry (the dynobj_set below).
            // An own `__proto__` DATA entry (shorthand `{__proto__}`
            // / defineProperty) shadows the setter the same way
            // (§10.1.9.2 OrdinarySet finds the own data property
            // first) — its write stays ordinary too.
            let hdr_flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            if hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO == 0
                && crate::prop_has::key_is(key, b"__proto__")
                && __torajs_dynobj_has(ptr, key as *const c_void) == 0
            {
                let boxed = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                __torajs_anyv_proto_member_set(recv, boxed);
                // The setter takes its own stake on the stored cell;
                // the caller's transferred reference dies here.
                drop_payload(tag, value);
                return;
            }
            let mut obj = ptr;
            __torajs_dynobj_set(&mut obj, key, tag, value);
            if obj != ptr {
                // Relocated block — the NaN-box cell encoding is the
                // pointer bits; transfer, no rc traffic (same object
                // identity, moved storage).
                *recv_slot = __torajs_anyv_box_from_pair(4, obj as i64);
            }
            return;
        }
        if cell_tag == Tag::RegExp as u16 && hint == ANY_RPROP_LAST_INDEX {
            let idx = match tag {
                2 => value as i64 as f64,
                // The f64 slot stores fractional values uncoerced
                // (`r.lastIndex = 2.9` reads back 2.9); ToLength
                // happens at the regex kernels' consumption sites.
                3 => f64::from_bits(value),
                _ => {
                    // Non-numeric lastIndex payloads are a recorded
                    // boundary (ES stores any value; the cell field
                    // is f64) — loud, not a silent 0.
                    reject(tag, value);
                    return;
                }
            };
            __torajs_regex_set_last_index(ptr, idx);
            return;
        }
        if cell_tag == Tag::Closure as u16 {
            // chunk C (RFC 20260711) — ES §20.2.4 `name` / `length`
            // are non-writable; tr programs are module-strict so the
            // assign throws (bun: "Attempted to assign to readonly
            // property."). Unconditional even after a delete — the
            // set then walks to `Function.prototype`'s own
            // non-writable pair and refuses the same way (bun
            // parity); recreates go through defineProperty (a
            // recorded follow-up).
            if crate::prop_has::key_is(key, b"name") || crate::prop_has::key_is(key, b"length") {
                drop_payload(tag, value);
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
                return;
            }
            let props_slot = ptr.cast::<u8>().add(MEMBER_SET_CLOSURE_PROPS_OFF) as *mut u64;
            let mut props = *props_slot as *mut c_void;
            if props.is_null() {
                props = __torajs_dynobj_alloc();
            }
            __torajs_dynobj_set(&mut props, key, tag, value);
            // First-write alloc and resize relocation both land the
            // fresh table back in the +24 slot; the closure cell
            // itself never moves.
            *props_slot = props as u64;
            return;
        }
        // RFC 20260714-objlit-accessor blade 7 — a struct accessor's
        // [[Set]] (the write mirror of blade 5's read). A setter runs
        // with the value; a GET-ONLY property throws (ES §10.1.9 — an
        // assignment whose [[Set]] is undefined fails, and a module is
        // strict). A plain data field keeps the reject below: a typed
        // struct through `any` cannot grow or rewrite a slot yet (RFC
        // 20260714-struct-dynamic-props).
        if cell_tag == Tag::Obj as u16 {
            let name_len = (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read();
            let name_bytes = key.cast::<u8>().add(STR_DATA_OFF);
            let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
            if crate::struct_probe::__torajs_struct_accessor_set(
                ptr, name_bytes, name_len, value_anyv,
            ) {
                // The setter borrowed the value out of argv; the write
                // path owns the payload it was handed.
                drop_payload(tag, value);
                return;
            }
        }
        if cell_tag == Tag::Arr as u16 {
            // RFC 20260712-arr-exotic-define chunk C — own-domain
            // routing for the dynamic string key. Pre-fix every
            // `o[k] = v` landed in the expando dynobj: an index key
            // never reached element storage (the write "succeeded"
            // but reads answered the old element), and a "length"
            // key missed the resize path.
            if hint == ANY_WPROP_ARR_LENGTH || crate::prop_has::key_is(key, b"length") {
                __torajs_arr_set_length_any(ptr, tag as i64, value as i64);
                return;
            }
            if let Some(idx) = crate::prop_has::canonical_index(key) {
                // An accessor index writes through its setter (RFC
                // 20260713 chunk C) — checked before the writable
                // gate, which would misread the pair entry's dead w
                // bit as a readonly data property.
                let pair = __torajs_arr_index_accessor(ptr, idx);
                if !pair.is_null() {
                    let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
                    let recv_anyv = __torajs_anyv_box_from_pair(4, ptr as i64);
                    if __torajs_accessor_invoke_setter(pair, recv_anyv, value_anyv) == 0 {
                        __torajs_throw_type_error(
                            c"Attempted to assign to readonly property.".as_ptr(),
                        );
                    }
                    return;
                }
                let flags = __torajs_arr_index_flags(ptr, idx);
                if flags & crate::prop_has::ARR_F_HOLE != 0 {
                    // A deleted index is absent — the set re-creates
                    // it as a fresh default data property (chunk C;
                    // the shadow upsert clears the hole sentinel).
                    __torajs_arr_index_set(ptr, idx as i64, tag, value);
                    __torajs_arr_index_revive(ptr, key as *mut c_void);
                    return;
                }
                if flags & 0x1 == 0 {
                    drop_payload(tag, value);
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr(),
                    );
                    return;
                }
                __torajs_arr_index_set(ptr, idx as i64, tag, value);
                return;
            }
            __torajs_arrprops_set(ptr, key, tag as i64, value as i64);
            return;
        }
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — a primitive-
        // wrapper receiver (`new Number()` / `new String()` /
        // `new Boolean()`) grew a `+16` lazy props slot; the first
        // `wrap.foo = X` allocates a fresh dynobj, subsequent sets
        // resize-relocate the same slot. Mirror of the closure-cell
        // path above — no cell-identity move (only the props slot's
        // stored pointer moves on grow). Unlocks the test262
        // `Object.create` primitive-wrapper mutation cluster and the
        // broader `wrap.foo = ...` face that was previously TypeErr.
        if cell_tag == Tag::NumberWrapper as u16
            || cell_tag == Tag::StringWrapper as u16
            || cell_tag == Tag::BooleanWrapper as u16
        {
            // §10.4.3 String Exotic — `"length"` and the in-range
            // code-unit indices are non-writable own properties:
            // [[Set]] answers false and strict assignment turns
            // that into a TypeError (§13.15.2; bun throws). The
            // pre-fix store landed in the expando dynobj, so the
            // dynamic-key read handed back the shadow value and
            // test262's isWritable probe saw a writable length.
            if cell_tag == Tag::StringWrapper as u16 && strwrapper_own_domain_key(ptr, key) {
                drop_payload(tag, value);
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
                return;
            }
            let props_slot = ptr.cast::<u8>().add(MEMBER_SET_WRAPPER_PROPS_OFF) as *mut u64;
            let mut props = *props_slot as *mut c_void;
            // §10.1.5.1 [[Set]] on a non-extensible wrapper rejects new
            // keys. The wrapper cell owns the `FLAG_NON_EXTENSIBLE`
            // bit (set by `Object.preventExtensions(w)`), not the
            // expando dynobj it lazily allocates — so `dynobj_set`'s
            // own gate can't cover this; check the wrapper header
            // directly here. Update to an existing expando key
            // (`w.foo = 2` after `w.foo = 1; preventExtensions(w)`)
            // stays allowed — matches bun-parity, mirror of the
            // dynobj_set / dynobj_define gates.
            let wrapper_flags = *(ptr.cast::<u8>().add(6) as *const u16);
            if wrapper_flags & FLAG_NON_EXTENSIBLE != 0 {
                let key_present = !props.is_null()
                    && __torajs_dynobj_has(props as *const c_void, key as *const c_void) != 0;
                if !key_present {
                    reject_non_extensible(tag, value);
                    return;
                }
            }
            if props.is_null() {
                props = __torajs_dynobj_alloc();
            }
            __torajs_dynobj_set(&mut props, key, tag, value);
            *props_slot = props as u64;
            return;
        }
        reject(tag, value);
    }
}
