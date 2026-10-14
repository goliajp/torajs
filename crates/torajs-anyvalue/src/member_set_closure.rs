//! `Tag::Closure` receiver arm of [`crate::member_set`] — the
//! non-writable reflection pair (§20.2.4 `name` / `length`), the
//! builtin-ctor `prototype` lock (§22.1.2.4 family) and the lazy
//! `+24` expando props slot. Split out of `member_set.rs` (rotation
//! 268 — Reflect.set 参数化前的余量腾挪, mechanical move).

use core::ffi::c_void;

use torajs_rc::{FLAG_FROZEN, FLAG_NON_EXTENSIBLE};

use crate::member_set::{
    __torajs_dynobj_alloc, __torajs_throw_type_error, drop_payload, dynobj_set_flavored,
};

unsafe extern "C" {
    /// torajs-rc — the one first-attach of a user closure's props bag
    /// (link-judged; see `torajs_rc::closure_entry`).
    fn __torajs_closure_props_attach(cell: *mut u8, props: *mut c_void);
    /// torajs-dynobj — own-entry presence probe (prop_has's kernel).
    fn __torajs_dynobj_has(obj: *const c_void, key: *const c_void) -> i32;
    /// torajs-dynobj — §10.1.6.3 define with explicit W/E/C
    /// (`flags_byte` low 3 = values, bits 3-5 = present, bit 6 =
    /// value present); consumes the value stake.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    /// torajs-buffer — §10.4.5.5 TypedArraySetElement (coerce first,
    /// store only on a valid index).
    fn __torajs_typedarray_index_set(
        recv: crate::nanbox::AnyValue,
        idx: f64,
        v: crate::nanbox::AnyValue,
    );
}

/// §10.1.9.2 step 2.b — the [[Extensible]] gate every lazy-bag arm
/// shares (Arr side props, struct, closure, wrapper, stateful bags):
/// a NEW key on a non-extensible receiver is refused, because the
/// create is what `[[Extensible]] = false` forbids. An existing
/// entry is not this gate's question — its own writable bit answers
/// inside `dynobj_set`, and `Object.freeze` clears that bit through
/// the bag walk (`__torajs_cell_expando_bag`). The receiver CELL
/// owns the header bit, not the dynobj it lazily allocates, which is
/// why `dynobj_set`'s gate cannot see it. Consumes the payload and
/// throws (flavored) when it refuses. Before this the arms each
/// carried their own copy: the closure one refused every key on a
/// frozen receiver with the readonly message (right answer, wrong
/// face for a new key), and the Arr one had no gate at all.
pub(crate) unsafe fn refuse_new_key_non_extensible(
    recv: *const c_void,
    key_present: bool,
    tag: u64,
    value: u64,
    throw_on_refusal: bool,
) -> bool {
    if key_present {
        return false;
    }
    let hflags = unsafe { (recv.cast::<u8>().add(6) as *const u16).read() };
    if hflags & FLAG_NON_EXTENSIBLE == 0 {
        return false;
    }
    unsafe {
        drop_payload(tag, value);
        if throw_on_refusal {
            __torajs_throw_type_error(
                c"Attempting to define property on object that is not extensible.".as_ptr(),
            );
        }
    }
    true
}

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const MEMBER_SET_CLOSURE_PROPS_OFF: usize = 24;

/// `torajs_rc::Tag::Str` — a property key cell is a Str or a Symbol.
const TAG_STR_KEY: u16 = 0;

/// Mark a write to a builtin-prototype singleton's expando against
/// the CELL, so the patch bitmap sees it. A no-op for every ordinary
/// closure (the address scan answers -1 inside) and for a symbol key
/// (the bitmap is mid-keyed, and mids are string names).
///
/// # Safety
/// `cell` is a live Closure cell; `key` is a live key cell.
unsafe fn note_builtin_proto_write(cell: *mut c_void, key: *const c_void) {
    unsafe {
        if key.is_null() || key.cast::<u8>().add(4).cast::<u16>().read() != TAG_STR_KEY {
            return;
        }
        let name = torajs_rc::str_wtf8::StrWtf8::of(key);
        torajs_rc::builtin_proto::__torajs_builtin_proto_note_own_write(
            cell,
            name.as_ptr(),
            name.len() as i64,
        );
    }
}

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
        // ArrayBuffer and DataView share +32 by construction.
        MEMBER_SET_ARRAYBUFFER_PROPS_OFF
    };
    unsafe { set_expando_member(ptr, off, key, tag, value, throw_on_refusal) }
}

/// Shared plain-assign expando write for cells whose own properties
/// live in a lazy props dynobj at `props_off` (Promise / ArrayBuffer
/// / TypedArray / Map / Set / Date / RegExp). Integrity-gated; first
/// write allocates the table.
pub(crate) unsafe fn set_expando_member(
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
        let present = !props.is_null() && __torajs_dynobj_has(props, key as *const c_void) != 0;
        // §10.1.9.2 — an own miss consults the [[Prototype]] chain
        // before the own create. Ahead of the integrity gate: freezing
        // bars the create, not the walk.
        if !present
            && let Some(handled) = crate::member_set_dynobj::inherited_set_handled(
                ptr,
                crate::nanbox::box_void_ptr(ptr),
                key,
                tag,
                value,
                throw_on_refusal,
            )
        {
            return handled;
        }
        if refuse_new_key_non_extensible(ptr, present, tag, value, throw_on_refusal) {
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
        // The read-only own faces; tr programs are module-strict, so
        // the assign throws (bun: "Attempted to assign to readonly
        // property."):
        //   - chunk C (RFC 20260711) — ES §20.2.4 `name` / `length`.
        //     Unconditional even after a delete — the set then walks
        //     to `Function.prototype`'s own non-writable pair and
        //     refuses the same way (bun parity); recreates go through
        //     defineProperty (a recorded follow-up);
        //   - §22.1.2.4 family — a builtin ctor cell's `prototype` is
        //     {[[Writable]]: false} (RFC 20260721 刀 11 G11); an
        //     ordinary closure keeps its writable prototype expando;
        //   - §6.1.5.1 — the Symbol ctor's well-known data statics
        //     (RFC 20260722 刀 2).
        if crate::prop_has::key_is(key, b"name")
            || crate::prop_has::key_is(key, b"length")
            || (crate::prop_has::key_is(key, b"prototype")
                && crate::method_value::ctor::ctor_tag_of_cell(ptr).is_some())
            || crate::method_value::symbol_static::is_wellknown_on_symbol_ctor(ptr, key)
            || crate::method_value::is_ctor_data_constant(ptr, key)
        {
            drop_payload(tag, value);
            if throw_on_refusal {
                __torajs_throw_type_error(c"Attempted to assign to readonly property.".as_ptr());
            }
            return 0;
        }
        let props_slot = ptr.cast::<u8>().add(MEMBER_SET_CLOSURE_PROPS_OFF) as *mut u64;
        let mut props = *props_slot as *mut c_void;
        // §10.1.9.2 — an own miss consults the [[Prototype]] chain
        // before the own create. Ahead of the integrity gate: freezing
        // bars the create, not the walk.
        // A function's own faces here are the expando and the virtual
        // `prototype` pair; `name` / `length` never reach this far.
        let in_bag = !props.is_null() && __torajs_dynobj_has(props, key as *const c_void) != 0;
        // The virtual `prototype` pair: an own key with no bag entry.
        let virtual_proto = !in_bag
            && crate::prop_has::key_is(key, b"prototype")
            && crate::closure_proto::fn_prototype_pair(ptr).is_some();
        // `fn_prototype_pair` is not a pure read: on a function with
        // no bag yet it MATERIALIZES the virtual pair — a fresh bag in
        // the +24 slot holding `prototype → {constructor: F}`. The
        // `props` read above predates that, and attaching a second bag
        // over it orphaned the first with its stake on `F`: every
        // `F.prototype = …` on a fresh function leaked the function
        // and both bags (1 KB per evaluation of a class with a
        // runtime-value parent, whose lowering is exactly that write).
        // `in_bag` keeps its meaning — the entry was not there BEFORE
        // this write — so the first write still defines with the
        // virtual pair's attributes, now over the materialized entry.
        props = *props_slot as *mut c_void;
        if virtual_proto {
            // The virtual pair has no bag entry to carry the writable
            // bit `Object.freeze` clears, so the header answers for
            // it: a frozen function's `prototype` is read-only.
            let hflags = (ptr.cast::<u8>().add(6) as *const u16).read();
            if hflags & FLAG_FROZEN != 0 {
                drop_payload(tag, value);
                if throw_on_refusal {
                    __torajs_throw_type_error(
                        c"Attempted to assign to readonly property.".as_ptr(),
                    );
                }
                return 0;
            }
        } else {
            // The walk is seeded at the cell, not at its prototype: an
            // own bag entry is met first, and an accessor there (a
            // static setter of a class lowered to a function value)
            // runs with the function as `this`; an own miss walks on
            // up the chain.
            if let Some(handled) = crate::member_set_dynobj::inherited_set_walk(
                Some(ptr as u64),
                crate::nanbox::box_void_ptr(ptr),
                key,
                tag,
                value,
                throw_on_refusal,
            ) {
                return handled;
            }
            if refuse_new_key_non_extensible(ptr, in_bag, tag, value, throw_on_refusal) {
                return 0;
            }
        }
        let first_attach = props.is_null();
        if first_attach {
            props = __torajs_dynobj_alloc();
        }
        // §20.2.4.3 — the virtual `prototype` is {writable: true,
        // enumerable: false, configurable: false}; its first write
        // moves it into the bag, and OrdinarySet keeps the attributes
        // it had (§10.1.9.2 step 2.c), so that write is a define with
        // those bits. The plain set's all-true default made a written
        // `prototype` enumerable and deletable.
        let wrote = if virtual_proto {
            // value present + W/E/C present + W only = 0x79.
            __torajs_dynobj_define(&mut props, key, tag, value, 0x79);
            1
        } else {
            dynobj_set_flavored(&mut props, key, tag, value, throw_on_refusal)
        };
        // §20.2.3 makes `Function.prototype` a built-in FUNCTION
        // object, so a `Function.prototype.call = f` patch lands in
        // THIS expando — and the note hook inside the dynobj set sees
        // only the expando, which is not the singleton, so the patch
        // bit stayed clear and every dispatcher kept answering the
        // native arm. torajs-arr notes the Arr-backed
        // `Array.prototype`'s writes the same way, for the same
        // reason: the cell is what carries the identity.
        if wrote != 0 {
            note_builtin_proto_write(ptr, key);
        }
        // First-write alloc and resize relocation both land the
        // fresh table back in the +24 slot; the closure cell
        // itself never moves. The first attach is the link-judged
        // entry (A5 seam evidence).
        if first_attach {
            __torajs_closure_props_attach(ptr.cast::<u8>(), props);
        } else {
            *props_slot = props as u64;
        }
        wrote
    }
}
