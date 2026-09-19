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
    /// torajs-dynobj — the narrow entry-table define (RFC
    /// 20260825-inject-narrow-define 刀 1). Takes its own key stake;
    /// the VALUE stake is the caller's to provide. (Signature mirrors
    /// the classmeta declaration — the lint insists they agree.)
    fn __torajs_dynobj_define_plain(
        obj_slot: *mut *mut c_void,
        key: *const u8,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
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
pub(crate) fn is_null_any(v: u64) -> bool {
    v == VALUE_NULL_IMM || v == 0
}

/// `DYNOBJ_HDR_FLAG_NON_EXTENSIBLE` mirror (torajs-dynobj layout).
pub(crate) const HDR_FLAG_NON_EXTENSIBLE: u16 = 1 << 8;

/// Closure-cell lazy props slot — mirror of torajs-core
/// `ssa_lower.rs::CLOSURE_PROPS_OFF`.
pub(crate) const CLOSURE_PROPS_OFF: usize = 24;

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

/// Why §10.1.2.1 answered false. bun words the two differently, and
/// the anyv faces pick the sentence by this.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProtoRefusal {
    /// Step 5 — the receiver is not extensible and V differs.
    NonExtensible,
    /// Step 8 — V's chain leads back to the receiver.
    Cycle,
    /// §10.4.7.1 — %Object.prototype% is an immutable-prototype
    /// exotic object.
    ImmutableProto,
}

/// The TypeError for a refusal (bun's sentences).
pub(crate) unsafe fn throw_proto_refusal(r: ProtoRefusal) {
    let msg = match r {
        ProtoRefusal::NonExtensible => c"Attempted to assign to readonly property.",
        ProtoRefusal::Cycle => c"cyclic __proto__ value",
        ProtoRefusal::ImmutableProto => c"Cannot set prototype of immutable prototype object",
    };
    unsafe { __torajs_throw_type_error(msg.as_ptr()) };
}

/// §10.1.2.1 steps 4-8 — refuse on non-extensible (unless same
/// value) and on a chain cycle; write otherwise. `obj` is the dynobj
/// that carries the `__proto__` entry; `receiver_flags` is the header
/// word of the RECEIVER whose [[Extensible]] the write asks about —
/// the same cell for an ordinary object, the closure cell (not its
/// props bag) for a function value. Before this a frozen function
/// re-parented freely, because the bag it writes through was never
/// the thing that got frozen.
pub(crate) unsafe fn ordinary_set_prototype_of(
    obj: *mut c_void,
    proto: u64,
    receiver_flags: u16,
) -> Result<(), ProtoRefusal> {
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
            return Ok(());
        }
        // §10.4.7.1 — %Object.prototype% is an immutable-prototype
        // exotic object: any non-same-value write refuses (the
        // same-value short-circuit above already admitted null →
        // null). Ordered after step 3 so `set.call(Object.prototype,
        // null)` stays silent per spec.
        if __torajs_builtin_proto_tag_of(obj) == OBJECT_PROTO_TAG {
            __torajs_str_drop(key);
            return Err(ProtoRefusal::ImmutableProto);
        }
        if receiver_flags & HDR_FLAG_NON_EXTENSIBLE != 0 {
            __torajs_str_drop(key);
            return Err(ProtoRefusal::NonExtensible);
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
                return Err(ProtoRefusal::Cycle);
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
        Ok(())
    }
}

/// The universal header word of a cell — its [[Extensible]] bit.
pub(crate) unsafe fn cell_flags(cell: *const c_void) -> u16 {
    unsafe { cell.cast::<u8>().add(6).cast::<u16>().read() }
}

/// The prologue's proto-chain wire (RFC 20260825-inject-narrow-define
/// 刀 4a) — `__proto_<Sub>.__proto__ = __proto_<Super>`, emitted by
/// `class_globals_register` as a Call to this kernel instead of a
/// generic member assign. The generic `__torajs_any_member_set` route
/// dispatches on receiver shape and consults the Annex B setter
/// machinery (same-value probe, extensibility, the chain cycle walk),
/// and that one statically-rooted reloc kept the whole member-set
/// world alive in every program (empty-program census: −5,780 text).
///
/// Caller contract (compile-time facts at the ONLY mint site): both
/// operands are the injection/desugar prologue's freshly minted plain
/// dynobjs — extensible, no existing `__proto__` entry — and the link
/// follows the compile-time class tree (acyclic, the parser refuses a
/// heritage cycle), so every step the general route probes is
/// statically settled.
///
/// # Safety
/// `sub` / `superp` box live `TAG_DYNOBJ` cells per the contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_proto_link_fresh(sub: u64, superp: u64) {
    unsafe {
        let obj = sub as *mut c_void;
        let parent = superp as *mut c_void;
        // The entry holds one stake on the parent (define_plain incs
        // the key itself; the value stake is the caller's to put up).
        __torajs_rc_inc(parent);
        let key = alloc_str_key(PROTO_SLOT_KEY);
        let mut slot = obj;
        __torajs_dynobj_define_plain(
            &mut slot,
            key,
            ANY_HEAP as u64,
            parent as u64,
            PROTO_SLOT_ATTRS,
        );
        __torajs_str_drop(key);
    }
}
