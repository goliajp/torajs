//! `Object.setPrototypeOf(O, proto)` + the `o.__proto__ = v` setter
//! (Annex B §B.2.2.1) — RFC 20260717-user-proto-chain knife 3.
//!
//! Both faces share §10.1.2.1 OrdinarySetPrototypeOf over the
//! `__proto__` simulation slot: same-value short-circuit, the
//! non-extensible refusal, the chain cycle walk (following the same
//! user-chain steps the knife-2 read path takes; a non-dynobj link
//! ends the walk — its implicit builtin chain cannot loop back into
//! a user object), then the write (a null proto deletes the entry
//! and sets the null-proto header bit; a cell clears the bit and
//! lands in the slot with its own +1).
//!
//! The two faces differ only in failure reporting, per spec:
//! `Object.setPrototypeOf` throws TypeError on an invalid proto arg
//! and on OrdinarySetPrototypeOf refusing (cycle / non-extensible);
//! the `__proto__` setter silently ignores an invalid value and
//! throws only on the refusal.
//!
//! Recorded boundary: non-dynobj receivers (Arr / Closure / wrapper
//! / struct cells) keep their builtin [[Prototype]] — the write is
//! ignored (spec would allow re-parenting exotic objects; the
//! per-shape proto slot is a follow-up knife).

use core::ffi::{c_char, c_void};

use crate::reflect::{
    ANY_HEAP, DYNOBJ_HDR_FLAG_NULL_PROTO, OBJECT_PROTO_TAG, PROTO_SLOT_ATTRS, PROTO_SLOT_KEY,
    TAG_CLOSURE, TAG_DYNOBJ, VALUE_NULL_IMM, alloc_str_key, heap_type_tag, is_cell_imm,
};

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_has(dynobj: *const c_void, key: *const u8) -> bool;
    fn __torajs_dynobj_get_tag(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_get_value(dynobj: *const c_void, key: *const u8) -> u64;
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_dynobj_delete(obj: *mut c_void, key: *const c_void) -> i32;
    fn __torajs_dynobj_mark_null_proto(obj: *mut c_void);
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    fn __torajs_dynobj_clear_null_proto(obj: *mut c_void);
    fn __torajs_throw_type_error(msg: *const c_char);
    fn __torajs_builtin_proto_tag_of(p: *const c_void) -> i64;
}

/// The two null encodings that reach these faces: the canonical
/// `VALUE_NULL_IMM` NaN-box and the SSA typed-Null pack (tag 0,
/// value 0 → raw 0 — the same pair `get_proto_of_any` accepts).
fn is_null_any(v: u64) -> bool {
    v == VALUE_NULL_IMM || v == 0
}

/// `DYNOBJ_HDR_FLAG_NON_EXTENSIBLE` mirror (torajs-dynobj layout).
const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
const CLOSURE_PROPS_OFF: usize = 24;

/// The expando dynobj that CARRIES a function value's user
/// [[Prototype]] link (405-01 substrate) — the closure cell grows no
/// new slot; the `\x00proto` simulation entry lands in the same lazy
/// props bag every other expando uses. First touch allocates and
/// parks it back at +24 (the dynobj cell itself never relocates, so
/// no later writeback is needed).
unsafe fn closure_proto_carrier(cell: *mut c_void) -> *mut c_void {
    unsafe {
        let slot = cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *mut u64;
        let mut props = *slot as *mut c_void;
        if props.is_null() {
            props = __torajs_dynobj_alloc();
            *slot = props as u64;
        }
        props
    }
}

/// The receiver's current user-proto link: the own `__proto__` cell
/// entry, or null for the null-proto shape / an absent entry (the
/// implicit %Object.prototype%).
unsafe fn current_proto(dynobj: *const c_void, key: *const u8) -> u64 {
    let flags = unsafe { dynobj.cast::<u8>().add(6).cast::<u16>().read() };
    if flags & DYNOBJ_HDR_FLAG_NULL_PROTO != 0 {
        return VALUE_NULL_IMM;
    }
    if !unsafe { __torajs_dynobj_has(dynobj, key) } {
        return VALUE_NULL_IMM;
    }
    let t = unsafe { __torajs_dynobj_get_tag(dynobj, key) } as i64;
    let v = unsafe { __torajs_dynobj_get_value(dynobj, key) };
    if t == ANY_HEAP && v != 0 {
        v
    } else {
        VALUE_NULL_IMM
    }
}

/// §10.1.2.1 steps 4-8 — refuse on non-extensible (unless same
/// value) and on a chain cycle; write otherwise. `true` = done.
pub(crate) unsafe fn ordinary_set_prototype_of(obj: *mut c_void, proto: u64) -> bool {
    unsafe {
        let proto = if is_null_any(proto) {
            VALUE_NULL_IMM
        } else {
            proto
        };
        let key = alloc_str_key(PROTO_SLOT_KEY);
        let cur = current_proto(obj, key);
        // Step 3 — SameValue(V, current) succeeds untouched, even on
        // a non-extensible receiver. `current_proto` reads an
        // absent-entry implicit chain as null, but its TRUE
        // [[Prototype]] is %Object.prototype% — comparing the raw
        // null made `setPrototypeOf(o, null)` on an implicit-chain
        // receiver a same-value no-op, so the null-proto bit was
        // never set and getPrototypeOf answered `{}` instead of
        // null. Map implicit → the %Object.prototype% cell for this
        // comparison only (the cycle walk keeps the raw read — an
        // implicit chain is all-builtin and can't reach a user
        // dynobj). %Object.prototype% itself is exempt: its real
        // [[Prototype]] IS null, and the same-value short-circuit is
        // what keeps `set.call(Object.prototype, null)` silent per
        // §10.4.7.1.
        let hdr_flags = obj.cast::<u8>().add(6).cast::<u16>().read();
        let cur = if cur == VALUE_NULL_IMM
            && hdr_flags & DYNOBJ_HDR_FLAG_NULL_PROTO == 0
            && __torajs_builtin_proto_tag_of(obj) != OBJECT_PROTO_TAG
        {
            let op = __torajs_get_builtin_prototype(OBJECT_PROTO_TAG);
            if op.is_null() {
                VALUE_NULL_IMM
            } else {
                op as u64
            }
        } else {
            cur
        };
        if proto == cur {
            __torajs_str_drop(key);
            return true;
        }
        // §10.4.7.1 — %Object.prototype% is an immutable-prototype
        // exotic object: any non-same-value write refuses (the
        // same-value short-circuit above already admitted null →
        // null). Ordered after step 3 so `set.call(Object.prototype,
        // null)` stays silent per spec.
        if __torajs_builtin_proto_tag_of(obj) == OBJECT_PROTO_TAG {
            __torajs_str_drop(key);
            return false;
        }
        let flags = obj.cast::<u8>().add(6).cast::<u16>().read();
        if flags & HDR_FLAG_NON_EXTENSIBLE != 0 {
            __torajs_str_drop(key);
            return false;
        }
        // Steps 6-8 — walk the would-be parent chain; finding the
        // receiver itself is the cycle refusal. A closure link hops
        // through its expando props bag — the dynobj that CARRIES its
        // user [[Prototype]] entry (405-01), and also the cell this
        // fn is asked to write when the receiver is a function value,
        // so the ptr_eq probe catches closure cycles too. Any other
        // non-dynobj link ends the walk (builtin implicit chains
        // cannot reach a user object).
        let mut cur_p = proto;
        while is_cell_imm(cur_p) {
            let mut cell = cur_p as *mut c_void;
            if heap_type_tag(cell) == TAG_CLOSURE {
                let props = *(cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64);
                if props == 0 {
                    break;
                }
                cell = props as *mut c_void;
            }
            if core::ptr::eq(cell, obj) {
                __torajs_str_drop(key);
                return false;
            }
            if heap_type_tag(cell) != TAG_DYNOBJ {
                break;
            }
            cur_p = current_proto(cell, key);
        }
        if is_null_any(proto) {
            __torajs_dynobj_delete(obj, key as *const c_void);
            __torajs_dynobj_mark_null_proto(obj);
        } else {
            __torajs_dynobj_clear_null_proto(obj);
            let cell = proto as *mut c_void;
            // The entry owns its reference (define transfers the
            // value; redefine drops the old stake). define — not set
            // — so a re-link after the null→delete path recreates
            // the entry with PROTO_SLOT_ATTRS' enumerable-clear.
            __torajs_rc_inc(cell);
            let mut slot = obj;
            __torajs_dynobj_define(
                &mut slot,
                key,
                ANY_HEAP as u64,
                cell as u64,
                PROTO_SLOT_ATTRS,
            );
        }
        __torajs_str_drop(key);
        true
    }
}

/// `Object.setPrototypeOf(O, proto)` — §20.1.2.21. Invalid proto is
/// a TypeError; a primitive O passes through untouched (step 3); a
/// refusal (cycle / non-extensible) is a TypeError.
///
/// # Safety
/// `obj` / `proto` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_set_prototype_of(obj: u64, proto: u64) {
    unsafe {
        if !is_null_any(proto) && !is_cell_imm(proto) {
            __torajs_throw_type_error(c"Prototype must be an object or null".as_ptr());
            return;
        }
        if !is_cell_imm(obj) {
            return;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // 405-01 substrate — a FUNCTION value re-parents through
            // its expando props bag (the extends lane's
            // `Object.setPrototypeOf(D, P)` static face).
            if heap_type_tag(cell) == TAG_CLOSURE {
                let carrier = closure_proto_carrier(cell);
                if !ordinary_set_prototype_of(carrier, proto) {
                    __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
                }
                return;
            }
            // A static-layout struct has no __proto__ simulation
            // slot — the write CANNOT take effect, and the silent
            // return read as success (rotation 154 probe:
            // getPrototypeOf(child) === base stayed false). Loud
            // TypeError, aligned with the Object.assign
            // struct-target boundary; real re-parenting needs the
            // variable-position any-promotion family (L3b).
            if heap_type_tag(cell) == crate::reflect::TAG_OBJ {
                __torajs_throw_type_error(
                    c"Cannot set prototype of a fixed-layout object".as_ptr(),
                );
                return;
            }
            // Recorded boundary — exotic receivers (Arr / wrapper)
            // keep their builtin [[Prototype]].
            return;
        }
        if !ordinary_set_prototype_of(cell, proto) {
            __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
        }
    }
}

/// `Reflect.setPrototypeOf(target, proto)` — §28.1.12 (rotation 266
/// 刀 R4). Same OrdinarySetPrototypeOf core as the Object flavor,
/// but a refusal (cycle / non-extensible / fixed-layout boundary)
/// answers 0 instead of throwing — the [[SetPrototypeOf]] boolean is
/// the Reflect return value (§10.1.2). An invalid proto still throws
/// (step 1 both flavors). The caller's strict IsObject gate runs
/// first, so a primitive target never reaches the pass-through arm.
///
/// # Safety
/// `obj` / `proto` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_reflect_set_prototype_of(obj: u64, proto: u64) -> i64 {
    unsafe {
        if !is_null_any(proto) && !is_cell_imm(proto) {
            __torajs_throw_type_error(c"Prototype must be an object or null".as_ptr());
            return 0;
        }
        if !is_cell_imm(obj) {
            return 0;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // 405-01 substrate — function values re-parent through
            // the expando carrier, same as the Object flavor.
            if heap_type_tag(cell) == TAG_CLOSURE {
                let carrier = closure_proto_carrier(cell);
                return i64::from(ordinary_set_prototype_of(carrier, proto));
            }
            // Fixed-layout struct / exotic (Arr / wrapper)
            // receivers cannot take a new [[Prototype]] in tr — the
            // Object flavor throws / silently keeps; the honest
            // Reflect spelling of "the write cannot take effect" is
            // false (recorded boundary, same family as the
            // rotation-154 struct probe).
            return 0;
        }
        i64::from(ordinary_set_prototype_of(cell, proto))
    }
}

/// Annex B §B.2.2.1 `set __proto__(v)` — an invalid v is silently
/// ignored (step 2 returns undefined); only the OrdinarySetPrototypeOf
/// refusal throws.
///
/// # Safety
/// `obj` / `proto` carry valid AnyValue bit patterns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_anyv_proto_member_set(obj: u64, proto: u64) {
    unsafe {
        if !is_null_any(proto) && !is_cell_imm(proto) {
            return;
        }
        if !is_cell_imm(obj) {
            return;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // 405-01 substrate — `D.__proto__ = P` on a function
            // value routes through the same expando carrier.
            if heap_type_tag(cell) == TAG_CLOSURE {
                let carrier = closure_proto_carrier(cell);
                if !ordinary_set_prototype_of(carrier, proto) {
                    __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
                }
            }
            return;
        }
        if !ordinary_set_prototype_of(cell, proto) {
            __torajs_throw_type_error(c"Cannot set prototype of this object".as_ptr());
        }
    }
}
