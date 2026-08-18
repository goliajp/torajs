//! `__torajs_any_length_get` / `__torajs_any_size_get` — `recv.length`
//! and `recv.size` where the receiver is an `any` value (RFC 20260704
//! S4 / C4-2), plus the closure-length walk they share. Carved out of
//! `index_any.rs` (file-size HARD RULE) — the element get/set half
//! stays there; this sibling owns the special-prop getters, the
//! `name_get.rs` dual.
//!
//! Both getters follow the OWNED return protocol: the returned box
//! holds its own reference (fresh i64 boxes are immediates; DynObj
//! probe pairs take `payload_rc_inc` before boxing), the caller's
//! ownership ledger (chunk 717 `owned_member_reads`) drops it.

use core::ffi::c_void;

use crate::index_any::{
    MIRROR_ARR_LEN_OFF, MIRROR_FLAG_SUBSTR_INLINE, MIRROR_STR_LEN_OFF, MIRROR_SUBSTR_LEN_OFF,
    utf16_units_of_utf8,
};
use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_cell, is_null, is_short_str, is_undefined,
    short_str_bytes, short_str_len,
};
use torajs_rc::Tag;

unsafe extern "C" {
    /// torajs-fnname — registry walk (chunk 716); NULL = miss. The
    /// `.length` arm reads the arity out param off a fn-decl entry.
    fn __torajs_fn_name_lookup(fn_addr: u64, out_len: *mut u32, out_arity: *mut u32) -> *const u8;
    /// torajs-dynobj — run an accessor entry's getter (owned answer).
    fn __torajs_accessor_invoke_getter(pair: *const c_void, recv_anyv: u64) -> u64;
    /// torajs-throw — pending-throw flag (1 = a throw is recorded).
    fn __torajs_throw_check() -> i64;
    /// torajs-str — release a heap Str/Substr reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError; returns
    /// normally (caller's throw-check propagates).
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — heap Str constructor (rc=1), used to build the
    /// literal "length" / "size" probe keys for DynObj receivers.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-dynobj — own-entry existence (disambiguates a stored
    /// undefined from absent; the G9d chain walk keys off absent).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — own-property probe pair ((5, 0) = absent →
    /// undefined by construction).
    fn __torajs_dynobj_get_tag(obj: *const c_void, key: *const c_void) -> u64;
    fn __torajs_dynobj_get_value(obj: *const c_void, key: *const c_void) -> u64;
    /// torajs-collections — live entry count (tombstones excluded);
    /// Set storage shares the Map runtime.
    fn __torajs_map_size(p: *const c_void) -> i64;
}

/// Accessor-entry sentinel in the dynobj probe's tag channel —
/// mirror of `method_call_dynobj.rs::ANY_ACCESSOR_TAG`.
const ANY_ACCESSOR_TAG: u64 = 6;

/// Resolve a DynObj own-property probe pair to an owned boxed value,
/// running an accessor entry's getter (§9.1.8 — observable; a pending
/// throw answers `undefined` and the caller's throw-check propagates).
/// A data pair takes `payload_rc_inc` before boxing — both arms answer
/// OWNED (RFC 20260713-accessor-void-kind blade 2; pre-fix the
/// `.length` / `.size` special-prop reads boxed the raw accessor pair
/// as data: the getter never ran). `pub(crate)`: `name_get.rs`'s
/// closure expando shadow shares it for the same accessor face.
pub(crate) unsafe fn box_probe_pair(dtag: u64, dval: u64, recv: AnyValue) -> AnyValue {
    unsafe {
        if dtag == ANY_ACCESSOR_TAG {
            let got = __torajs_accessor_invoke_getter(dval as *const c_void, recv);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
            return got;
        }
        crate::payload_rc_inc(dtag as i64, dval as i64);
        crate::nanbox_encode::__torajs_anyv_box_from_pair(dtag as i64, dval as i64)
    }
}

/// `recv.length` where the receiver is an `any` value
/// (RFC 20260704 S4).
///
/// - `null` / `undefined` → catchable TypeError.
/// - strings (ShortStr / Str / Substr) → UTF-16 code-unit count.
/// - `Tag::Arr` → element count.
/// - `Tag::DynObj` → own-property probe for the literal key
///   `"length"` (a user `{ length: 5 }` answers 5; absence answers
///   `undefined`).
/// - other primitives / heap tags → `undefined` (no such property).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_length_get(recv: AnyValue) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if is_short_str(recv) {
        let len = short_str_len(recv) as usize;
        let bytes = short_str_bytes(recv);
        let payload = &bytes[..len];
        let units = if payload.iter().all(|b| *b < 0x80) {
            len as i64
        } else {
            utf16_units_of_utf8(payload)
        };
        return crate::nanbox_encode::__torajs_anyv_box_i64(units);
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        // RFC 20260716 刀 3 — primitive-wrapper view-through
        // (see `wrapper_view_through`). Number/Boolean consult their
        // own expando `length` first (刀 9 G2c — `obj.length = 2` on
        // `new Boolean(false)` is an own data property, §10.1.8.1
        // own-before-proto), then fall to primitive immediates
        // (undefined for `.length`); StringWrapper's inherent
        // non-writable length wins over any expando — its inner Str
        // cell routes to the Str arm below.
        if crate::member_get::is_wrapper_tag(tag) && tag != Tag::StringWrapper as u16 {
            let props = crate::member_get::wrapper_props(ptr);
            if !props.is_null() {
                let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
                let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let dval = __torajs_dynobj_get_value(props, key as *const c_void);
                let present = dtag != 5 || __torajs_dynobj_has(props, key as *const c_void) != 0;
                __torajs_str_drop(key as *mut c_void);
                if present {
                    return box_probe_pair(dtag, dval, recv);
                }
            }
        }
        if let Some(inner) = crate::wrapper_view_through::resolve_inner_recv(ptr, tag) {
            return __torajs_any_length_get(inner);
        }
        if tag == Tag::Arr as u16 {
            let n = *(ptr.cast::<u8>().add(MIRROR_ARR_LEN_OFF) as *const u64);
            return crate::nanbox_encode::__torajs_anyv_box_i64(n as i64);
        }
        if tag == Tag::Str as u16 {
            let flags = (ptr.cast::<u8>().add(6) as *const u16).read();
            let n = if flags & MIRROR_FLAG_SUBSTR_INLINE != 0 {
                *(ptr.cast::<u8>().add(MIRROR_SUBSTR_LEN_OFF) as *const u64) as i64
            } else {
                *(ptr.cast::<u8>().add(MIRROR_STR_LEN_OFF) as *const u32) as i64
            };
            return crate::nanbox_encode::__torajs_anyv_box_i64(n);
        }
        if tag == Tag::DynObj as u16 {
            // Own-property probe for the literal key "length" — a
            // user `{ length: 5 }` through any answers its value;
            // absence answers (5, 0) = undefined by construction; an
            // accessor entry runs its getter (box_probe_pair).
            let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
            let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            // 刀 6 G9d — a truly absent own entry continues along the
            // user [[Prototype]] chain (§10.1.8.1 OrdinaryGet):
            // `Object.create(arr).length` answers the Arr parent's
            // inherent length. The recursion covers grandparents
            // (member_get knife-2 shape — a heap cell's pointer bits
            // ARE its box encoding).
            if dtag == 5 && __torajs_dynobj_has(ptr, key as *const c_void) == 0 {
                if let Some(parent) = crate::member_get_own::user_proto_cell(ptr) {
                    __torajs_str_drop(key as *mut c_void);
                    return __torajs_any_length_get(parent);
                }
                // Function.prototype's virtual own `length` 0
                // (§20.2.3, RFC 20260722 刀 3).
                if let Some((mtag, mval)) = crate::method_support_proto_meta::builtin_proto_own_meta(
                    ptr,
                    key as *const c_void,
                ) {
                    __torajs_str_drop(key as *mut c_void);
                    return crate::nanbox_encode::__torajs_anyv_box_from_pair(
                        mtag as i64,
                        mval as i64,
                    );
                }
            }
            __torajs_str_drop(key as *mut c_void);
            return box_probe_pair(dtag, dval, recv);
        }
        if tag == Tag::Obj as u16 {
            // Struct cell (an inline object literal's anon struct, or
            // a class instance) — the member_get chunk-744 struct arm
            // specialized to `.length`: own field probe first, then
            // the +24 expando dict, then undefined. Pre-fix this tag
            // fell through to the tail undefined, so String.raw's
            // array-like walk counted an inline `{length: n, …}` raw
            // as 0.
            if let Some((dtag, dval)) = crate::struct_probe::struct_field_pair_bytes(ptr, b"length")
            {
                return box_probe_pair(dtag, dval, recv);
            }
            let props = crate::member_get_layout::struct_props(ptr);
            if !props.is_null() {
                let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
                let present = __torajs_dynobj_has(props, key as *const c_void) != 0;
                let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let dval = __torajs_dynobj_get_value(props, key as *const c_void);
                __torajs_str_drop(key as *mut c_void);
                if present {
                    return box_probe_pair(dtag, dval, recv);
                }
            }
            return VALUE_UNDEFINED;
        }
        if tag == Tag::Closure as u16 {
            // §10.1.6.3 — a define/write that landed in the expando
            // is a REDEFINED own `length` and shadows every virtual
            // source below: the tombstone recreate (chunk C), the
            // reflection-surface cells (RFC 20260721 刀 2), and a
            // `defineProperty(f, "length", …)` data or accessor form
            // (pre-fix the registry arity answered first and the
            // define was invisible — the rotation-347 recorded
            // silent-wrong). Present-undefined answers undefined
            // (the wrapper arm's `has` form).
            let props = crate::member_get::closure_props(ptr);
            if !props.is_null() {
                let key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
                let dtag = __torajs_dynobj_get_tag(props, key as *const c_void);
                let dval = __torajs_dynobj_get_value(props, key as *const c_void);
                let present = dtag != 5 || __torajs_dynobj_has(props, key as *const c_void) != 0;
                __torajs_str_drop(key as *mut c_void);
                if present {
                    return box_probe_pair(dtag, dval, recv);
                }
            }
            // chunk C (RFC 20260711) — a tombstoned virtual `length`
            // with no expando recreate continues along [[Prototype]]:
            // %Function.prototype% carries an own `length` 0 (§20.2.3),
            // so `delete f.length; f.length` answers 0 like bun (the
            // formerly recorded undefined edge).
            if crate::member_get::header_flag(ptr, torajs_rc::FLAG_FN_LENGTH_DELETED) {
                return crate::nanbox_encode::__torajs_anyv_box_i64(0);
            }
            // chunk 715 — a reified builtin method cell answers its
            // ES-spec arity; chunk 716 — an ordinary closure walks
            // the fn-addr registry (the entry's former pad slot now
            // carries the fn-decl arity); chunk 719 — a bound cell
            // answers `max(0, targetLength - boundArgc)` recursively
            // (ES §20.2.3.2 BoundFunctionCreate). A registry miss
            // (arrow / anon closures never register) stays undefined
            // — the binding-context NamedEvaluation face is a
            // recorded follow-up, and a bound-of-unregistered target
            // inherits the undefined (recorded edge; ES would answer
            // 0 via the has-length fallback).
            if let Some(l) = closure_length_of(ptr) {
                return crate::nanbox_encode::__torajs_anyv_box_i64(l);
            }
        }
    }
    VALUE_UNDEFINED
}

/// `.length` metadata read for external consumers — torajs-meta's
/// gOPD Closure arm (RFC 20260711-closure-reflection chunk B) builds
/// the virtual `length` descriptor from this. `-1` = no metadata
/// (the descriptor stays undefined, mirroring the `.length` read).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_closure_length(ptr: *mut c_void) -> i64 {
    unsafe { closure_length_of(ptr) }.unwrap_or(-1)
}

/// ES `length` of a closure-tagged cell: method-cell arity, bound
/// cell (recursive over its target, partial args subtract), or the
/// fn-addr registry's fn-decl arity. `None` = no metadata (arrow /
/// anon closures off the registry).
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
unsafe fn closure_length_of(ptr: *mut c_void) -> Option<i64> {
    unsafe {
        if let Some(arity) = crate::method_value::builtin_method_arity(ptr) {
            return Some(arity as i64);
        }
        // Reified class-accessor face (RFC 20260718-accessor-reify
        // 刀 2) — spec lengths ride the extended cell slot.
        if let Some((_, len)) = crate::method_value_class::class_accessor_meta(ptr) {
            return Some(len as i64);
        }
        if let Some((kind, target, bargc)) = crate::method_bind::bound_cell_meta(ptr) {
            let tlen = if kind == 0 {
                let (mid, fam) = crate::method_bind::unpack_builtin_target(target);
                crate::method_value::builtin_method_arity_for(mid, fam).map(|a| a as i64)
            } else {
                closure_length_of(target as *mut c_void)
            };
            return tlen.map(|l| (l - bargc as i64).max(0));
        }
        // B6c — a class-method face resolves its adapter's registry
        // row (the ES method arity).
        let fn_addr = crate::method_value_class::registry_addr(ptr);
        let mut name_len: u32 = 0;
        let mut arity: u32 = 0;
        let name_ptr = __torajs_fn_name_lookup(fn_addr, &mut name_len, &mut arity);
        if !name_ptr.is_null() {
            return Some(arity as i64);
        }
        None
    }
}

/// `recv.size` where the receiver is an `any` value
/// (Any-method-call RFC 20260704 C4-2).
///
/// - `null` / `undefined` → catchable TypeError.
/// - `Tag::Map` / `Tag::Set` → live entry count (the ES §24.1.3.10 /
///   §24.2.3.9 `size` accessors) via `__torajs_map_size`.
/// - `Tag::DynObj` → own-property probe for the literal key `"size"`
///   (a user `{ size: 42 }` answers 42; absence answers `undefined`).
/// - everything else (strings, arrays, primitives, other heap tags)
///   → `undefined` (no such property).
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_size_get(recv: AnyValue) -> AnyValue {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            __torajs_throw_type_error(c"cannot read properties of null or undefined".as_ptr());
        }
        return VALUE_UNDEFINED;
    }
    if !is_cell(recv) {
        return VALUE_UNDEFINED;
    }
    let ptr = as_void_ptr(recv);
    unsafe {
        let tag = (ptr.cast::<u8>().add(4) as *const u16).read();
        if tag == Tag::Map as u16 || tag == Tag::Set as u16 {
            return crate::nanbox_encode::__torajs_anyv_box_i64(__torajs_map_size(ptr));
        }
        if tag == Tag::DynObj as u16 {
            // Same probe shape as the `.length` DynObj arm above —
            // an accessor entry runs its getter (box_probe_pair).
            let key = __torajs_str_alloc(c"size".as_ptr() as *const u8, 4);
            let dtag = __torajs_dynobj_get_tag(ptr, key as *const c_void);
            let dval = __torajs_dynobj_get_value(ptr, key as *const c_void);
            __torajs_str_drop(key as *mut c_void);
            return box_probe_pair(dtag, dval, recv);
        }
    }
    VALUE_UNDEFINED
}
