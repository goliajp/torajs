//! `Object.getPrototypeOf(any)` + the `o.__proto__` member read
//! (Annex B §B.2.2.1) — split out of `reflect.rs` (file-size limit;
//! this crate's null-proto-flag + primitive-ToObject work pushed the
//! parent past 500). The descriptor faces stay in the parent.
//!
//! Symbols and ABI are unchanged — the extern names are what
//! ssa_lower declares (`__torajs_anyv_get_proto_of_any` /
//! `__torajs_anyv_proto_member_get`).

use core::ffi::c_void;

use crate::reflect::{
    ANY_HEAP, BOOLEAN_PROTO_TAG, DYNOBJ_HDR_FLAG_NULL_PROTO, NUMBER_PROTO_TAG, OBJECT_PROTO_TAG,
    SHORT_STR_TOP16, STRING_PROTO_TAG, TAG_ARR, TAG_CLOSURE, TAG_DYNOBJ, TAG_OBJ, TOP_16_MASK,
    VALUE_FALSE_IMM, VALUE_NULL_IMM, VALUE_TRUE_IMM, VALUE_UNDEFINED_IMM, alloc_str_key,
    box_pair_imm, heap_type_tag, is_cell_imm,
};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    // torajs-rc — lazy `<Ctor>.prototype` singleton by builtin tag /
    // the reverse pointer→tag probe (compared, never dereferenced).
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
}

/// A builtin prototype singleton as an owned AnyValue cell, or null
/// when it has never been allocated (which cannot happen — the
/// getter allocates on demand).
///
/// # Safety
/// `tag` is a valid builtin-proto tag.
unsafe fn proto_singleton(tag: i64) -> u64 {
    let p = unsafe { __torajs_get_builtin_prototype(tag) };
    if p.is_null() {
        return VALUE_NULL_IMM;
    }
    unsafe { __torajs_rc_inc(p) };
    p as u64
}

/// Reads the DYNOBJ null-proto header bit (bit 6 of the +6 u16 flags).
///
/// # Safety
/// `dynobj` points to a valid TAG_DYNOBJ heap object.
#[inline]
unsafe fn dynobj_is_null_proto(dynobj: *const c_void) -> bool {
    let flags = unsafe { (dynobj.cast::<u8>().add(6) as *const u16).read() };
    flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0
}

/// Reads a dynobj's own `__proto__` data slot (rc_inc'd on heap
/// payloads so the caller owns the reference), or `None` when the
/// object has no own `__proto__` entry. tr stores an ordinary
/// object's [[Prototype]] AS its own `__proto__` entry, so for a
/// non-null-proto object this slot IS the prototype; for a null-proto
/// object the entry (if any) is plain data (a `(?<__proto__>.)`
/// named capture, a `defineProperty`) and the caller must NOT treat
/// it as [[Prototype]].
///
/// # Safety
/// `dynobj` points to a valid TAG_DYNOBJ heap object.
unsafe fn dynobj_own_proto(dynobj: *const c_void) -> Option<u64> {
    let k = unsafe { alloc_str_key(b"__proto__") };
    if !unsafe { __torajs_dynobj_has(dynobj, k) } {
        unsafe { __torajs_str_drop(k) };
        return None;
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k) } as i64;
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k) } as i64;
    unsafe { __torajs_str_drop(k) };
    // rc_inc heap payload — caller owns the returned reference.
    if v_tag == ANY_HEAP && v_val != 0 {
        // SAFETY: ANY_HEAP slot holds a valid heap pointer.
        unsafe { __torajs_rc_inc(v_val as *mut c_void) };
    }
    Some(box_pair_imm(v_tag, v_val))
}

/// Annex B §B.2.2.1 — the `o.__proto__` READ, which is the same
/// [[Prototype]] answer with one difference: the getter lives ON
/// Object.prototype, so an object that does not inherit from it does
/// not have the getter either. `Object.create(null).__proto__` is a
/// plain absent property (`undefined`), not `null` — that is what
/// `Object.getPrototypeOf` answers.
///
/// # Safety
/// `v` carries a valid AnyValue bit pattern; cell case must point to
/// a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_proto_member_get(v: u64) -> u64 {
    if is_cell_imm(v) {
        let cell = v as *const c_void;
        // SAFETY: is_cell_imm guarantees a live heap pointer.
        if unsafe { heap_type_tag(cell) } == TAG_DYNOBJ && unsafe { dynobj_is_null_proto(cell) } {
            // A null-proto object does not inherit Object.prototype's
            // `__proto__` accessor, so `o.__proto__` is an ordinary
            // property read: an own `__proto__` data slot (e.g. a
            // `(?<__proto__>.)` named capture on a RegExp groups
            // object) wins; otherwise the property is simply absent →
            // undefined (NOT null — that is Object.getPrototypeOf's
            // answer).
            return unsafe { dynobj_own_proto(cell) }.unwrap_or(VALUE_UNDEFINED_IMM);
        }
    }
    unsafe { __torajs_anyv_get_proto_of_any(v) }
}

/// AnyValue-immediate `Object.getPrototypeOf(any)` — reads the
/// `__proto__` slot from the wrapped dynobj and returns a NaN-box
/// `AnyValue` immediate. Identity-preserving (the returned cell
/// wraps the SAME dynobj pointer the parent prototype was stored
/// at, so `getPrototypeOf(C.prototype) === B.prototype`).
///
/// # Safety
///
/// `v` carries a valid AnyValue bit pattern; cell case must
/// point to a valid heap object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_get_proto_of_any(v: u64) -> u64 {
    if !is_cell_imm(v) {
        // §B.2.2.1 step 1 ToObject — a primitive answers its
        // wrapper's prototype (the wrapper itself is unobservable
        // through this getter, so no object needs minting). Only
        // null / undefined have none. `"x".__proto__` is the common
        // one: a short string is a NaN-box immediate, not a cell, so
        // it used to fall out here as null.
        let proto_tag = if v == VALUE_TRUE_IMM || v == VALUE_FALSE_IMM {
            BOOLEAN_PROTO_TAG
        } else if (v & TOP_16_MASK) == SHORT_STR_TOP16 {
            STRING_PROTO_TAG
        } else if v == VALUE_NULL_IMM || v == VALUE_UNDEFINED_IMM || v == 0 {
            return VALUE_NULL_IMM;
        } else {
            // Every remaining immediate encoding is a number.
            NUMBER_PROTO_TAG
        };
        return unsafe { proto_singleton(proto_tag) };
    }
    let dynobj = v as *const c_void;
    // A builtin prototype answers before anything reads its shape:
    // every one of them inherits from %Object.prototype% (§20.1.3 —
    // whose own [[Prototype]] is the chain's null root), and that
    // holds no matter which cell shape backs it. Asking the tag
    // table instead would send `Array.prototype` — an Arr cell —
    // through the TAG_ARR arm below and hand back *itself*.
    let bp_tag = unsafe { __torajs_builtin_proto_tag_of(dynobj) };
    if bp_tag == OBJECT_PROTO_TAG {
        return VALUE_NULL_IMM;
    }
    if bp_tag >= 0 {
        return unsafe { proto_singleton(OBJECT_PROTO_TAG) };
    }
    // SAFETY: cell pointer to valid heap object per invariant.
    let tag = unsafe { heap_type_tag(dynobj) };
    if tag != TAG_DYNOBJ {
        // Static-layout class instance (Tag::Obj) — the class tag
        // lives in the universal +8 header slot (0 for plain
        // type-alias structs, so unregistered shapes keep the null
        // answer). Route through the same tag→proto table the typed
        // `Object.getPrototypeOf` lowering reads, so the answer is
        // identical across the Any and Obj tiers (RFC 20260713
        // blade 5: `Object.getPrototypeOf(g()) === g.prototype`).
        if tag == TAG_OBJ {
            let class_tag = unsafe { dynobj.cast::<u8>().add(8).cast::<i64>().read() };
            return unsafe { crate::classmeta::__torajs_anyv_proto_get(class_tag) };
        }
        // RFC 20260713-array-proto-residual blade 3 — builtin-tagged
        // cells answer their `<Ctor>.prototype` singleton per
        // §10.1.1 (an Array's [[Prototype]] IS Array.prototype, so
        // `getPrototypeOf(xs) === Array.prototype` holds). Tags with
        // no proto singleton (struct / iterator internals) keep the
        // null answer (recorded boundary).
        let proto_tag = match tag {
            TAG_ARR => 2,
            0 => 3, // Str / Substr view
            TAG_CLOSURE => 13,
            4 => 7,   // RegExp
            5 => 8,   // Date
            7 => 5,   // Symbol
            10 => 6,  // BigInt
            15 => 11, // Map
            19 => 12, // Set
            _ => -1,
        };
        if proto_tag >= 0 {
            return unsafe { proto_singleton(proto_tag) };
        }
        return VALUE_NULL_IMM;
    }
    // A null-proto object's [[Prototype]] IS null — and its own
    // `__proto__` entry, if present, is plain data (a `(?<__proto__>.)`
    // named capture, a `defineProperty`), never the prototype pointer.
    // Check the header bit BEFORE reading the entry so that data is not
    // mistaken for [[Prototype]] (`Object.getPrototypeOf(groups)` where
    // `groups` came from `/(?<__proto__>.)/` must answer null, not the
    // capture value).
    if unsafe { dynobj_is_null_proto(dynobj) } {
        return VALUE_NULL_IMM;
    }
    // §10.1.1 — tr stores an ordinary object's [[Prototype]] as its own
    // `__proto__` entry: present → that parent; absent →
    // %Object.prototype% (before the null-proto bit existed, an absent
    // entry answered null, so `Object.getPrototypeOf({}) ===
    // Object.prototype` was false).
    if let Some(own) = unsafe { dynobj_own_proto(dynobj) } {
        return own;
    }
    unsafe { proto_singleton(OBJECT_PROTO_TAG) }
}
