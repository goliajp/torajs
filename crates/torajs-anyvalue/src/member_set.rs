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

use torajs_rc::{ANY_RPROP_LAST_INDEX, Tag};

use crate::nanbox::{AnyValue, as_void_ptr, is_cell};
use crate::nanbox_encode::__torajs_anyv_box_from_pair;
use crate::nanbox_ffi::__torajs_anyv_rc_dec;

unsafe extern "C" {
    /// torajs-dynobj — realloc-on-grow set; the slot receives the
    /// possibly-relocated block pointer.
    pub(crate) fn __torajs_dynobj_set(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    );
    /// torajs-dynobj — the Reflect.set flavor of the entry write:
    /// refusal answers 0 instead of recording the TypeError.
    fn __torajs_dynobj_set_soft(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
    ) -> i64;
    /// torajs-regex — `re.lastIndex` setter.
    fn __torajs_regex_set_last_index(re: *mut c_void, idx: f64);
    /// torajs-regex — verbatim non-numeric lastIndex store (§22.2.4.1
    /// any-slot; TRANSFER — the boxed value's heap stake moves to the
    /// cell).
    fn __torajs_regex_last_index_store_boxed(re: *mut c_void, v: u64);
    /// torajs-dynobj — fresh empty table for the first closure
    /// expando write.
    pub(crate) fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-dynobj — probe whether a key already lives in the
    /// entry table (1 = live entry). Used by the wrapper
    /// `[[Extensible]] = false` gate to distinguish new-key writes
    /// (throw) from updates (allowed).
    pub(crate) fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-throw — record a pending catchable TypeError.
    pub(crate) fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
}

/// Release the lowering's +1 on a heap payload that no arm consumed.
pub(crate) unsafe fn drop_payload(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_anyv_rc_dec(__torajs_anyv_box_from_pair(4, value as i64)) };
    }
}

/// Non-assignable receiver refusal — flavored (R3-style): the strict
/// assignment records the TypeError, the Reflect.set flavor answers
/// `0` silently. Both drop the caller's +1.
unsafe fn reject(tag: u64, value: u64, throw_on_refusal: bool) -> i64 {
    unsafe {
        drop_payload(tag, value);
        if throw_on_refusal {
            __torajs_throw_type_error(c"cannot assign to a property of this any value".as_ptr());
        }
    }
    0
}

/// The `Tag::Obj` (class instance / nominal struct) arm of
/// [`any_member_set_impl`] — extracted so the dispatcher stays under
/// the function cap when the Proxy arm went in.
///
/// The order is §10.1.9 OrdinarySet's: an own accessor, then a
/// layout DATA field, then the +24 expando dict — and before an
/// expando is CREATED, the class prototype chain gets its say, so a
/// reified accessor up there writes through its setter instead of
/// being shadowed by a fresh own key.
///
/// `None` means "this arm did not claim the write" (the layout does
/// carry the name, or an own accessor is present but get-only) and
/// the caller falls through to its loud reject.
///
/// # Safety
/// `ptr` is a live `Tag::Obj` cell that `recv` boxes; `key` a live
/// Str cell; the `(tag, value)` pair transfers in.
unsafe fn set_obj_member(
    ptr: *mut c_void,
    recv: AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> Option<i64> {
    unsafe {
        let name_len = (key.cast::<u8>().add(STR_LEN_OFF) as *const u32).read();
        let name_bytes = key.cast::<u8>().add(STR_DATA_OFF);
        let value_anyv = __torajs_anyv_box_from_pair(tag as i64, value as i64);
        if crate::struct_probe::__torajs_struct_accessor_set(ptr, name_bytes, name_len, value_anyv)
        {
            // The setter borrowed the value out of argv; the write
            // path owns the payload it was handed.
            drop_payload(tag, value);
            return Some(1);
        }
        // RFC 20260718-error-message-own-prop — a class-layout
        // DATA field takes the store when the payload matches the
        // slot's type (Any / Str / I64 / F64 / Bool; frozen gate
        // inside). A mismatch falls through to the loud reject —
        // a typed slot never silently coerces.
        if crate::struct_error_msg::struct_data_field_set(ptr, key, tag, value) {
            return Some(1);
        }
        // RFC 20260714-struct-dynamic-props blade 2 — a name the
        // layout doesn't carry lands in the +24 expando dynobj
        // (lazily allocated; blade 1 zeroed the slot at every
        // alloc site). §10.1.5.1 [[Set]] on a non-extensible
        // struct rejects NEW keys — the struct header owns the
        // flag, not the expando dict, so the gate sits here
        // (wrapper-arm mirror); an update to a live expando key
        // stays allowed. dynobj_set consumes the caller's +1 and
        // writes the possibly-relocated block back through the
        // slot.
        if crate::struct_probe::struct_field_pair(ptr, key).is_none()
            && !crate::struct_probe::struct_accessor_key(ptr, key)
        {
            let hdr_flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            let props_slot = ptr
                .cast::<u8>()
                .add(crate::member_get_layout::OBJ_PROPS_OFF)
                as *mut *mut c_void;
            // Rotation 441 (3c) — §10.1.9.2: an own miss (layout,
            // own accessor, expando dict all silent) consults the
            // CLASS prototype chain before the own create — a
            // runtime-computed accessor reified on `__proto_<C>`
            // writes through its setter with this receiver (or
            // refuses get-only), instead of minting an own expando
            // entry that would shadow the getter on every later
            // read. An own expando key stays an ordinary own
            // write — OrdinarySet never consults the chain for it.
            let own_expando = !(*props_slot).is_null()
                && __torajs_dynobj_has(*props_slot, key as *const c_void) != 0;
            if !own_expando
                && let Some(r) = crate::member_set_dynobj::inherited_set_from_class_proto(
                    ptr,
                    recv,
                    key,
                    tag,
                    value,
                    throw_on_refusal,
                )
            {
                return Some(r);
            }
            if hdr_flags & torajs_rc::FLAG_NON_EXTENSIBLE != 0 {
                let key_present = !(*props_slot).is_null()
                    && __torajs_dynobj_has(*props_slot, key as *const c_void) != 0;
                if !key_present {
                    drop_payload(tag, value);
                    if throw_on_refusal {
                        __torajs_throw_type_error(
                            c"Attempting to define property on object that is not extensible."
                                .as_ptr(),
                        );
                    }
                    return Some(0);
                }
            }
            if (*props_slot).is_null() {
                *props_slot = __torajs_dynobj_alloc();
            }
            __torajs_dynobj_set(props_slot, key, tag, value);
            // The instance now owns a key its layout never
            // mentions, so a compile-time member list is no longer
            // the whole story — the fact every unfolding reader
            // gates on.
            let flags = ptr.cast::<u8>().add(6) as *mut u16;
            *flags |= torajs_rc::FLAG_OBJ_EXPANDO;
            return Some(1);
        }
        None
    }
}

/// Refusal AFTER the value was already consumed — the Proxy arm
/// hands the transfer to the trap (or to the target's own set core)
/// before it learns the answer, so this shell only picks the flavor
/// of "no" and never touches the payload again.
unsafe fn refuse_set(throw_on_refusal: bool) -> i64 {
    if throw_on_refusal {
        unsafe {
            __torajs_throw_type_error(c"proxy 'set' trap returned falsish".as_ptr());
        }
    }
    0
}

/// See module doc. `hint` carries the compile-time member-name
/// Str-cell payload offsets — mirror of `struct_probe.rs` (the key is
/// a live Str cell; blade 7 reads its bytes to resolve the accessor).
pub(crate) const STR_LEN_OFF: usize = 8;
pub(crate) const STR_DATA_OFF: usize = 16;

// §10.1.9.2 OrdinarySet chain walk + the DynObj own-entry write live
// in [`crate::member_set_dynobj`] (rotation 354 file-size split); the
// re-export keeps `member_set_symbol`'s import path canonical.
pub(crate) use crate::member_set_dynobj::inherited_set_handled;

/// Flavor split over the torajs-dynobj entry-write kernel — the
/// strict flavor keeps the historical void extern (a refusal records
/// the pending TypeError inside), the Reflect.set flavor routes
/// through the `_soft` twin and surfaces its success i64.
///
/// # Safety
/// Same contract as `__torajs_dynobj_set`.
pub(crate) unsafe fn dynobj_set_flavored(
    obj_slot: *mut *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        if throw_on_refusal {
            __torajs_dynobj_set(obj_slot, key, tag, value);
            1
        } else {
            __torajs_dynobj_set_soft(obj_slot, key, tag, value)
        }
    }
}

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
    unsafe { any_member_set_impl(recv_slot, key, tag, value, hint, true) };
}

/// §28.1.13 Reflect.set flavor — a [[Set]] refusal answers `0`
/// instead of recording the strict-assignment TypeError; a handled
/// write answers `1`. Same R3-style parameterization as
/// `__torajs_any_prop_delete_soft` / `__torajs_dynobj_set_soft`.
///
/// # Safety
/// Same contract as [`__torajs_any_member_set`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_member_set_soft(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
) -> i64 {
    unsafe { any_member_set_impl(recv_slot, key, tag, value, hint, false) }
}

/// Flavor-carrying core of the two shells above — `throw_on_refusal`
/// picks §13.15.2 strict-assignment (record the TypeError) or
/// §28.1.13 Reflect.set (answer false). Returns 1 = written / setter
/// ran, 0 = refused.
unsafe fn any_member_set_impl(
    recv_slot: *mut AnyValue,
    key: *mut c_void,
    tag: u64,
    value: u64,
    hint: i64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        let recv = *recv_slot;
        if !is_cell(recv) {
            return reject(tag, value, throw_on_refusal);
        }
        let ptr = as_void_ptr(recv);
        let cell_tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        // §10.5.9 — a Proxy answers the write itself, symbol keys
        // included (RFC 20260823-proxy-substrate 刀 2). A refusal
        // becomes whichever "no" this shell was built for.
        if cell_tag == torajs_rc::Tag::Proxy as u16 {
            return match crate::proxy_ops::set(ptr, key, tag, value, recv) {
                Ok(true) => 1,
                Ok(false) => refuse_set(throw_on_refusal),
                Err(()) => 0,
            };
        }
        // §6.1.7 — a symbol key takes its own lane; every arm below is
        // gated on a property NAME.
        if crate::member_get_symbol::key_is_symbol(key) {
            return crate::member_set_symbol::symbol_member_set(
                recv_slot,
                recv,
                ptr,
                cell_tag,
                key,
                tag,
                value,
                throw_on_refusal,
            );
        }
        if cell_tag == Tag::DynObj as u16 {
            return crate::member_set_dynobj::set_dynobj_member(
                recv_slot,
                recv,
                ptr,
                key,
                tag,
                value,
                throw_on_refusal,
            );
        }
        // The hint is the compile-time interned fast path; a DYNAMIC
        // key spelling `lastIndex` (`re[k] = v`) misses it, so the
        // byte probe backstops (§22.2.4.1 — same property either way).
        if cell_tag == Tag::RegExp as u16
            && (hint == ANY_RPROP_LAST_INDEX || crate::prop_has::key_is(key, b"lastIndex"))
        {
            let idx = match tag {
                2 => value as i64 as f64,
                // The f64 slot stores fractional values uncoerced
                // (`r.lastIndex = 2.9` reads back 2.9); ToLength
                // happens at the regex kernels' consumption sites.
                3 => f64::from_bits(value),
                _ => {
                    // §22.2.4.1 — lastIndex is an ordinary
                    // {writable: true} data property: a non-numeric
                    // value stores VERBATIM in the cell's boxed
                    // overflow slot (ToLength happens at the
                    // exec-entry consumption). The pair's +1 heap
                    // stake transfers to the cell.
                    let v =
                        crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
                    __torajs_regex_last_index_store_boxed(ptr, v);
                    return 1;
                }
            };
            __torajs_regex_set_last_index(ptr, idx);
            return 1;
        }
        if cell_tag == Tag::Closure as u16 {
            return crate::member_set_closure::set_closure_member(
                ptr,
                key,
                tag,
                value,
                throw_on_refusal,
            );
        }
        // Promise receiver — plain-assign expando write into the +32
        // bag the get channel probes (member_set_closure doc).
        if cell_tag == Tag::Promise as u16 {
            return crate::member_set_closure::set_promise_member(
                ptr,
                key,
                tag,
                value,
                throw_on_refusal,
            );
        }
        // RFC 20260714-objlit-accessor blade 7 — a struct accessor's
        // [[Set]] (the write mirror of blade 5's read). A setter runs
        // with the value; a GET-ONLY property throws (ES §10.1.9 — an
        // assignment whose [[Set]] is undefined fails, and a module is
        // strict). A plain data field keeps the reject below: a typed
        // struct through `any` cannot grow or rewrite a slot yet (RFC
        // 20260714-struct-dynamic-props).
        if cell_tag == Tag::Obj as u16
            && let Some(r) = set_obj_member(ptr, recv, key, tag, value, throw_on_refusal)
        {
            return r;
        }
        if cell_tag == Tag::Arr as u16 {
            return crate::member_set_arr::set_arr_member(
                ptr,
                key,
                tag,
                value,
                hint,
                throw_on_refusal,
            );
        }
        // RFC 20260716 刀 5 (rotation 121 chunk 4) — a primitive-
        // wrapper receiver (`new Number()` / `new String()` /
        // `new Boolean()`) grew a `+16` lazy props slot; arm body in
        // [`crate::member_set_wrapper::set_wrapper_member`].
        if cell_tag == Tag::NumberWrapper as u16
            || cell_tag == Tag::StringWrapper as u16
            || cell_tag == Tag::BooleanWrapper as u16
        {
            return crate::member_set_wrapper::set_wrapper_member(
                ptr,
                cell_tag,
                key,
                tag,
                value,
                throw_on_refusal,
            );
        }
        reject(tag, value, throw_on_refusal)
    }
}
