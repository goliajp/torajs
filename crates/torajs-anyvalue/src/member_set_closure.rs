//! `Tag::Closure` receiver arm of [`crate::member_set`] — the
//! non-writable reflection pair (§20.2.4 `name` / `length`), the
//! builtin-ctor `prototype` lock (§22.1.2.4 family) and the lazy
//! `+24` expando props slot. Split out of `member_set.rs` (rotation
//! 268 — Reflect.set 参数化前的余量腾挪, mechanical move).

use core::ffi::c_void;

use torajs_rc::{FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED};

use crate::member_set::{
    __torajs_dynobj_alloc, __torajs_throw_type_error, drop_payload, dynobj_set_flavored,
};

unsafe extern "C" {
    /// torajs-dynobj — own-entry presence probe (prop_has's kernel).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-buffer — §10.4.5.5 TypedArraySetElement (coerce first,
    /// store only on a valid index).
    fn __torajs_typedarray_index_set(
        recv: crate::nanbox::AnyValue,
        idx: f64,
        v: crate::nanbox::AnyValue,
    );
}

/// §7.3 integrity gate shared by the closure / promise expando arms.
/// `Object.freeze` / `seal` / `preventExtensions` mark only the
/// receiver HEADER for non-DynObj cells (`extensible_reflect` —
/// their property surface lives here, not in a DynObj bucket), so
/// the expando write must consult it: a frozen receiver refuses
/// every assign (§10.1.9 — all own props non-writable, no growth);
/// a sealed / non-extensible one refuses only NEW keys (an existing
/// data entry stays writable under seal). Pre-fix `Object.freeze(f)`
/// left `f.a = 1` mutating the frozen function while
/// `Object.isFrozen(f)` answered true.
unsafe fn expando_integrity_refuses(
    recv: *const c_void,
    props: *const c_void,
    key: *const c_void,
) -> bool {
    let hflags = unsafe { (recv.cast::<u8>().add(6) as *const u16).read() };
    if hflags & FLAG_FROZEN != 0 {
        return true;
    }
    if hflags & (FLAG_SEALED | FLAG_NON_EXTENSIBLE) != 0 {
        return props.is_null() || unsafe { __torajs_dynobj_has(props, key) } == 0;
    }
    false
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const MEMBER_SET_CLOSURE_PROPS_OFF: usize = 24;

/// Promise-cell lazy props slot — mirror of torajs-dynobj
/// `layout.rs::PROMISE_PROPS_OFF` (+24 is the callback list).
const MEMBER_SET_PROMISE_PROPS_OFF: usize = 32;

/// `Tag::Promise` receiver — the plain-assign twin of the
/// defineProperty arm's `define_into_expando(.., PROMISE_PROPS_OFF,
/// ..)` (RFC 20260810-sloppy-goal-arguments L3b ⑦: `p.foo = v` never
/// landed in the bag the get channel reads). A promise instance is an
/// ordinary object (§27.2) with no own reflection pair — `name` /
/// `length` / `then` are all prototype surface, so every key takes
/// the expando write; an own `then` stored here shadows the builtin
/// through the get arm's bag-first probe.
///
/// # Safety
/// `ptr` is a live `Tag::Promise` cell; `key` is a live Str cell;
/// `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn set_promise_member(
    ptr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        set_expando_member(
            ptr,
            MEMBER_SET_PROMISE_PROPS_OFF,
            key,
            tag,
            value,
            throw_on_refusal,
        )
    }
}

/// Buffer-family cell lazy props slots — mirrors of torajs-buffer
/// `arraybuffer.rs::PROPS_OFF` / `typedarray.rs::PROPS_OFF` (and of
/// `member_get_layout`'s read-side pair).
const MEMBER_SET_ARRAYBUFFER_PROPS_OFF: usize = 32;
const MEMBER_SET_TYPEDARRAY_PROPS_OFF: usize = 40;

/// `Tag::ArrayBuffer` / `Tag::TypedArray` receiver — the plain-assign
/// half of the view's ordinary-object face (§25.1 / §23.2): every
/// non-index key takes the expando write the get channel's bag-first
/// probe reads back. Numeric element stores never reach here — the
/// index-assign kernel owns §10.4.5.5 (`index_any_set`).
///
/// # Safety
/// `ptr` is a live buffer-family cell of `cell_tag`; `key` is a live
/// Str cell; `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn set_buffer_member(
    ptr: *mut c_void,
    cell_tag: u16,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    // §10.4.5.5 — a canonical numeric spelling on a typed array is
    // the ELEMENT face, never the bag: `ta["0"] = v` coerces and
    // stores like `ta[0] = v`, and an invalid index still coerces
    // (observably — tonumber-value-throws) and then drops the store.
    // The kernel owns that order.
    if cell_tag == torajs_rc::Tag::TypedArray as u16
        && let Some(n) = unsafe { crate::member_get_buffer::canonical_numeric_key(key) }
    {
        unsafe {
            let recv = crate::nanbox_encode::__torajs_anyv_box_pointer(ptr);
            let boxed = crate::nanbox_encode::__torajs_anyv_box_from_pair(tag as i64, value as i64);
            __torajs_typedarray_index_set(recv, n, boxed);
            crate::nanbox_ffi::__torajs_anyv_rc_dec(boxed);
        }
        return 1;
    }
    let off = if cell_tag == torajs_rc::Tag::TypedArray as u16 {
        MEMBER_SET_TYPEDARRAY_PROPS_OFF
    } else {
        MEMBER_SET_ARRAYBUFFER_PROPS_OFF
    };
    unsafe { set_expando_member(ptr, off, key, tag, value, throw_on_refusal) }
}

/// Shared plain-assign expando write for cells whose own properties
/// live in a lazy props dynobj at `props_off` (Promise / ArrayBuffer
/// / TypedArray). Integrity-gated; first write allocates the table.
unsafe fn set_expando_member(
    ptr: *mut c_void,
    props_off: usize,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
        let props_slot = ptr.cast::<u8>().add(props_off) as *mut u64;
        let mut props = *props_slot as *mut c_void;
        if expando_integrity_refuses(ptr, props, key) {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        if props.is_null() {
            props = __torajs_dynobj_alloc();
        }
        let wrote = dynobj_set_flavored(&mut props, key, tag, value, throw_on_refusal);
        // First-write alloc and resize relocation both land the
        // fresh table back in the slot; the receiver cell itself
        // never moves.
        *props_slot = props as u64;
        wrote
    }
}

/// `Tag::Closure` receiver — chunk C (RFC 20260711) reflection-pair
/// refusals plus the lazy expando write. Flavored (R3-style):
/// refusals throw or answer 0 per `throw_on_refusal`.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell; `key` is a live Str cell;
/// `(tag, value)` carries the caller's +1 on heap payloads.
pub(crate) unsafe fn set_closure_member(
    ptr: *mut c_void,
    key: *mut c_void,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> i64 {
    unsafe {
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
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        // §22.1.2.4 family — a builtin ctor cell's `prototype`
        // is {[[Writable]]: false} (RFC 20260721 刀 11 G11); an
        // ordinary closure keeps its writable prototype expando.
        if crate::prop_has::key_is(key, b"prototype")
            && crate::method_value::ctor::ctor_tag_of_cell(ptr).is_some()
        {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        // §6.1.5.1 — the Symbol ctor's well-known data statics
        // are {writable: false}; module-strict assign throws
        // (RFC 20260722 刀 2).
        if crate::method_value::symbol_static::is_wellknown_on_symbol_ctor(ptr, key) {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        let props_slot = ptr.cast::<u8>().add(MEMBER_SET_CLOSURE_PROPS_OFF) as *mut u64;
        let mut props = *props_slot as *mut c_void;
        if expando_integrity_refuses(ptr, props, key) {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        if props.is_null() {
            props = __torajs_dynobj_alloc();
        }
        let wrote = dynobj_set_flavored(&mut props, key, tag, value, throw_on_refusal);
        // First-write alloc and resize relocation both land the
        // fresh table back in the +24 slot; the closure cell
        // itself never moves.
        *props_slot = props as u64;
        wrote
    }
}
