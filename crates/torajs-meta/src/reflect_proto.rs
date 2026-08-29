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
    ANY_ACCESSOR, ANY_HEAP, BOOLEAN_PROTO_TAG, DYNOBJ_HDR_FLAG_NULL_PROTO, NUMBER_PROTO_TAG,
    OBJECT_PROTO_TAG, PROTO_SLOT_ATTRS, PROTO_SLOT_KEY, SHORT_STR_TOP16, STRING_PROTO_TAG, TAG_ARR,
    TAG_CLOSURE, TAG_DYNOBJ, TAG_OBJ, TOP_16_MASK, VALUE_FALSE_IMM, VALUE_NULL_IMM, VALUE_TRUE_IMM,
    VALUE_UNDEFINED_IMM, alloc_str_key, box_pair_imm, heap_type_tag, is_cell_imm,
};

/// `torajs_rc::FLAG_SUBCLASSED` mirror (flags bit 0, RFC 20260730
/// blade 1) — exotic cell minted as a user-class instance.
const FLAG_SUBCLASSED: u16 = 1;

/// `torajs_rc::FLAG_ARR_ARGUMENTS` mirror (Tag::Arr-private bit 1).
const ARR_FLAG_ARGUMENTS: u16 = 1 << 1;

/// `torajs_rc::FLAG_FN_GENERATOR` mirror (Tag::Closure-private
/// header-flags bit 3, RFC 20260721 刀 2).
const FLAG_FN_GENERATOR_BIT: u16 = 1 << 3;

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF` (405-01: the expando dynobj is
/// what carries a re-parented function value's [[Prototype]] link).
const CLOSURE_PROPS_OFF: usize = 24;

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    /// torajs-anyvalue — §10.5.1 [[GetPrototypeOf]] on a Proxy.
    fn __torajs_proxy_get_prototype_of(v: u64) -> u64;
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    // torajs-rc — lazy `<Ctor>.prototype` singleton by builtin tag /
    // the reverse pointer→tag probe (compared, never dereferenced).
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
    // torajs-rc — a builtin prototype's own [[Prototype]] slot (-1 =
    // the null root), and the slot an iterator cell inherits from.
    fn __torajs_proto_parent_tag(tag: i64) -> i64;
    fn __torajs_iter_cell_proto_tag(ptr: *const c_void, tag: i64) -> i64;
    // torajs-dynobj — DefineOwnProperty kernel (resize relocates
    // through the slot; the create-link insert is the fresh dict's
    // first entry, so the block cannot grow) + the
    // Object.create(null) header bit. define (not set) so the
    // simulation entry carries PROTO_SLOT_ATTRS' enumerable-clear.
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
}

/// `Object.create(proto)` §20.1.2.2 step 2 — link the validated proto
/// onto the fresh dynobj (RFC 20260717-user-proto-chain knife 1):
/// a heap-cell proto lands in the internal [`PROTO_SLOT_KEY`] slot
/// (the entry takes an owned +1, keeping the parent alive; identity-
/// preserving so `Object.getPrototypeOf(child) === parent`); a null
/// proto — static literal or a runtime-Any null, closing the
/// recorded residual — sets the null-prototype header bit instead.
/// Every other shape was rejected by
/// `__torajs_object_create_check_proto` before this runs (primitives
/// and undefined throw), so the fall-through is unreachable by
/// construction and deliberately a no-op.
///
/// # Safety
/// `obj` is the freshly allocated dynobj (live, zero entries);
/// `proto` carries a valid AnyValue bit pattern.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_object_create_link_proto(obj: *mut c_void, proto: u64) {
    if proto == VALUE_NULL_IMM {
        unsafe { __torajs_dynobj_mark_null_proto(obj) };
        return;
    }
    if !is_cell_imm(proto) {
        return;
    }
    unsafe {
        let cell = proto as *mut c_void;
        // The entry owns its reference; define transfers the value
        // (fresh insert incs only the key).
        __torajs_rc_inc(cell);
        let k = alloc_str_key(PROTO_SLOT_KEY);
        let mut slot = obj;
        __torajs_dynobj_define(&mut slot, k, ANY_HEAP as u64, cell as u64, PROTO_SLOT_ATTRS);
        __torajs_str_drop(k);
    }
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

/// Reads a dynobj entry under `key_bytes` (rc_inc'd on heap payloads
/// so the caller owns the reference), or `None` when absent. Two
/// callers, two keys: [`PROTO_SLOT_KEY`] IS the [[Prototype]] link
/// (simulation-slot key separation — the internal entry can never be
/// spelled by a user property name), and the user-spellable
/// `__proto__` is plain own DATA (a shorthand `{__proto__}`, a
/// `(?<__proto__>.)` named capture, a `defineProperty`).
///
/// # Safety
/// `dynobj` points to a valid TAG_DYNOBJ heap object.
unsafe fn dynobj_entry(dynobj: *const c_void, key_bytes: &[u8]) -> Option<u64> {
    let k = unsafe { alloc_str_key(key_bytes) };
    if !unsafe { __torajs_dynobj_has(dynobj, k) } {
        unsafe { __torajs_str_drop(k) };
        return None;
    }
    let v_tag = unsafe { __torajs_dynobj_get_tag(dynobj, k) } as i64;
    let v_val = unsafe { __torajs_dynobj_get_value(dynobj, k) } as i64;
    unsafe { __torajs_str_drop(k) };
    // An ACCESSOR entry is not a data shadow — the `__proto__`
    // member-read caller falls through to `get_proto_of_any`, which
    // IS the injected root getter's semantics (RFC
    // 20260718-accessor-reify 刀 1: %Object.prototype% now carries
    // the Annex B pair as a real own entry). A user-defined accessor
    // shadow keeps the same fallthrough (recorded boundary — its own
    // getter is not invoked here).
    if v_tag == ANY_ACCESSOR as i64 {
        return None;
    }
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
        let tag = unsafe { heap_type_tag(cell) };
        if tag == TAG_DYNOBJ {
            // §10.1.8.1 OrdinaryGet — an own `__proto__` DATA entry
            // (shorthand `{__proto__}`, a `(?<__proto__>.)` named
            // capture, a `defineProperty`) shadows the inherited
            // accessor on every dynobj, so the read answers the
            // stored value, not the [[Prototype]].
            if let Some(own) = unsafe { dynobj_entry(cell, b"__proto__") } {
                return own;
            }
            if unsafe { dynobj_is_null_proto(cell) } {
                // No inherited accessor — the property is simply
                // absent → undefined (NOT null — that is
                // Object.getPrototypeOf's answer).
                return VALUE_UNDEFINED_IMM;
            }
        } else if tag == TAG_OBJ {
            // Same own-first shadow for the struct-typed literal
            // shape — a static layout carrying an own `__proto__`
            // data field answers the field.
            if let Some(own) =
                unsafe { crate::struct_reflect::struct_own_field_anyv(cell, b"__proto__") }
            {
                return own;
            }
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
    // §10.5.1 — a Proxy answers its own [[Prototype]] (RFC
    // 20260823-proxy-substrate 刀 5).
    if unsafe { heap_type_tag(dynobj) } == crate::reflect::TAG_PROXY {
        return unsafe { __torajs_proxy_get_prototype_of(v) };
    }
    // A builtin prototype answers before anything reads its shape:
    // every one of them inherits from %Object.prototype% (§20.1.3 —
    // whose own [[Prototype]] is the chain's null root), and that
    // holds no matter which cell shape backs it. Asking the tag
    // table instead would send `Array.prototype` — an Arr cell —
    // through the TAG_ARR arm below and hand back *itself*.
    let bp_tag = unsafe { __torajs_builtin_proto_tag_of(dynobj) };
    if bp_tag >= 0 {
        // Not %Object.prototype% flatly any more: the five per-family
        // iterator prototypes sit UNDER %Iterator.prototype%
        // (§23.1.5.2 et al). `proto_parent_tag` is the one home for
        // which parent each slot has, and `-1` is the null root.
        let parent = unsafe { __torajs_proto_parent_tag(bp_tag) };
        if parent < 0 {
            return VALUE_NULL_IMM;
        }
        return unsafe { proto_singleton(parent) };
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
            let r = unsafe { crate::classmeta::__torajs_anyv_proto_get(class_tag) };
            if r != VALUE_NULL_IMM {
                return r;
            }
            // §10.1.1 — a plain struct-typed literal (class_tag 0 /
            // unregistered) is an ordinary object; its [[Prototype]]
            // is %Object.prototype%, not null (RFC
            // 20260718-accessor-reify 刀 1: `get.call({a: 1})` must
            // answer the root, matching the dynobj-lane implicit
            // chain).
            return unsafe { proto_singleton(OBJECT_PROTO_TAG) };
        }
        // RFC 20260730 blade 1 — an exotic cell minted as a subclass
        // instance answers ITS CLASS's prototype, not the builtin
        // singleton: `getPrototypeOf(new C()) === C.prototype` for
        // `class C extends Array`. Same class_tag → proto route as
        // the TAG_OBJ arm, identity from the blade-0 side table;
        // plain builtin cells (flag clear) fall through unchanged.
        {
            let flags = unsafe { dynobj.cast::<u8>().add(6).cast::<u16>().read() };
            if flags & FLAG_SUBCLASSED != 0 {
                let class_tag =
                    unsafe { crate::subclass_instance::__torajs_subclass_class_tag(dynobj) };
                if class_tag >= 0 {
                    let r = unsafe { crate::classmeta::__torajs_anyv_proto_get(class_tag) };
                    if r != VALUE_NULL_IMM {
                        return r;
                    }
                }
            }
        }
        // RFC 20260713-array-proto-residual blade 3 — builtin-tagged
        // cells answer their `<Ctor>.prototype` singleton per
        // §10.1.1 (an Array's [[Prototype]] IS Array.prototype, so
        // `getPrototypeOf(xs) === Array.prototype` holds). Tags with
        // no proto singleton (struct / iterator internals) keep the
        // null answer (recorded boundary).
        // RFC 20260721 刀 2 — a generator-factory cell's
        // [[Prototype]] is %GeneratorFunction.prototype% (§27.3.2),
        // the genfn trio singleton (already owned on return); every
        // other closure keeps %Function.prototype% below.
        if tag == TAG_CLOSURE {
            // 405-01 substrate — a re-parented function value answers
            // its user [[Prototype]] link from the expando carrier
            // (the `\x00proto` entry `setPrototypeOf` wrote there);
            // an explicit null answers null. Untouched closures fall
            // to the builtin faces below.
            let props = unsafe { *(dynobj.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) }
                as *const c_void;
            if !props.is_null() {
                if unsafe { dynobj_is_null_proto(props) } {
                    return VALUE_NULL_IMM;
                }
                if let Some(own) = unsafe { dynobj_entry(props, PROTO_SLOT_KEY) } {
                    return own;
                }
            }
            let flags = unsafe { dynobj.cast::<u8>().add(6).cast::<u16>().read() };
            if flags & FLAG_FN_GENERATOR_BIT != 0 {
                return unsafe { crate::genfn::__torajs_genfn_proto(0) };
            }
        }
        // §10.4.4.7 step 2 — an arguments materialization's
        // [[Prototype]] is %Object.prototype%, not Array.prototype
        // (the 10.6-5-1 assert; FLAG_ARR_ARGUMENTS mirror, Tag::Arr-
        // private bit 1).
        if tag == TAG_ARR {
            let flags = unsafe { dynobj.cast::<u8>().add(6).cast::<u16>().read() };
            if flags & ARR_FLAG_ARGUMENTS != 0 {
                return unsafe { proto_singleton(OBJECT_PROTO_TAG) };
            }
        }
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
            12 => 16, // WeakMap
            13 => 17, // WeakSet
            11 => 18, // WeakRef
            // RFC 20260716 刀 15 — primitive-wrapper cells (刀 2 alloc
            // tags 21/22/23) return the corresponding
            // `<Ctor>.prototype` singleton per §10.1.1: the wrapper's
            // [[Prototype]] IS the ctor's `.prototype`, matching bun's
            // `Object.getPrototypeOf(new String()) === String.prototype`.
            21 => 0, // NumberWrapper  → Number.prototype
            22 => 3, // StringWrapper  → String.prototype
            23 => 4, // BooleanWrapper → Boolean.prototype
            // The five per-family iterator prototypes, one link below
            // %Iterator.prototype% — `iter_cell_proto_tag` is what
            // knows how to get from three cell tags to five slots.
            // This used to answer %Iterator.prototype% flatly, a
            // recorded one-hop-shorter chain; the missing link is also
            // where §23.1.5.2 keeps `@@toStringTag`, so `[1].values()`
            // badged "[object Object]".
            16 | 17 | 25 => unsafe { __torajs_iter_cell_proto_tag(dynobj, tag as i64) },
            // RFC 20260823-typedarray-substrate — the buffer family:
            // ArrayBuffer at 19, DataView after the per-kind block,
            // and a typed array at 20 + its element kind (the 刀 4
            // slot layout), read off the cell since a static match
            // cannot see the discriminant. Recorded boundary: the
            // chain above a per-kind prototype is %Object.prototype%
            // — no %TypedArray%.prototype intermediate exists yet,
            // the same one-hop-shorter shape as the iterator protos.
            27 => 19, // ArrayBuffer → ArrayBuffer.prototype
            28 => {
                // Kind byte at +32 (torajs-buffer
                // `typedarray.rs::KIND_OFF` mirror).
                let kind = unsafe { dynobj.cast::<u8>().add(32).read() } as i64;
                20 + kind
            }
            29 => 32, // DataView → DataView.prototype
            _ => -1,
        };
        if proto_tag >= 0 {
            return unsafe { proto_singleton(proto_tag) };
        }
        return VALUE_NULL_IMM;
    }
    // A null-proto object's [[Prototype]] IS null (the internal slot
    // entry is deleted when the null-proto bit is set, so the order
    // here is belt-and-suspenders, not load-bearing).
    if unsafe { dynobj_is_null_proto(dynobj) } {
        return VALUE_NULL_IMM;
    }
    // §10.1.1 — tr stores an ordinary object's [[Prototype]] in the
    // internal PROTO_SLOT_KEY entry (user-unspellable, so an own
    // `__proto__` data property can never be mistaken for the link):
    // present → that parent; absent → %Object.prototype% (before the
    // null-proto bit existed, an absent entry answered null, so
    // `Object.getPrototypeOf({}) === Object.prototype` was false).
    if let Some(own) = unsafe { dynobj_entry(dynobj, PROTO_SLOT_KEY) } {
        return own;
    }
    unsafe { proto_singleton(OBJECT_PROTO_TAG) }
}
