//! Which road a `[[SetPrototypeOf]]` takes, by receiver kind — the
//! three runtime faces (`Object.setPrototypeOf`, `Reflect.setPrototypeOf`,
//! the Annex B `__proto__` setter) and the per-kind re-parents they
//! dispatch to. §10.1.2.1 OrdinarySetPrototypeOf itself lives in
//! `reflect_proto_set`; this file answers only which receiver reaches
//! it through which carrier, and which receivers keep their chain.

use core::ffi::{c_char, c_void};

use crate::classmeta::obj_proto::OBJ_PROPS_OFF;
use crate::reflect::{
    PROTO_SLOT_KEY, TAG_CLOSURE, TAG_DYNOBJ, TAG_OBJ, VALUE_NULL_IMM, alloc_str_key, heap_type_tag,
    is_cell_imm,
};
use crate::reflect_proto_set::{
    CLOSURE_PROPS_OFF, HDR_FLAG_NON_EXTENSIBLE, ProtoRefusal, cell_flags, is_null_any, link_proto,
    ordinary_set_prototype_of, throw_proto_refusal,
};

unsafe extern "C" {
    /// torajs-rc — the one first-attach of a user closure's props bag
    /// (link-judged; see `torajs_rc::closure_entry`).
    fn __torajs_closure_props_attach(cell: *mut u8, props: *mut c_void);
    /// torajs-rc — the one first-attach of a struct instance's props
    /// dict (link-judged, same contract as the closure entry).
    fn __torajs_obj_props_attach(cell: *mut u8, props: *mut c_void);
    fn __torajs_str_drop(s: *mut u8);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    /// torajs-anyvalue — §10.5.2 [[SetPrototypeOf]] on a Proxy.
    fn __torajs_proxy_set_prototype_of(obj: u64, proto: u64) -> i64;
    /// torajs-throw — non-zero iff a throw is in flight.
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const c_char);
}

/// The expando dynobj that CARRIES a function value's user
/// [[Prototype]] link (405-01 substrate) — the closure cell grows no
/// new slot; the `\x00proto` simulation entry lands in the same lazy
/// props bag every other expando uses. First touch allocates and
/// parks it back at +24 (the dynobj cell itself never relocates, so
/// no later writeback is needed).
unsafe fn closure_proto_carrier(cell: *mut c_void) -> *mut c_void {
    unsafe {
        let mut props = *(cell.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const u64) as *mut c_void;
        if props.is_null() {
            props = __torajs_dynobj_alloc();
            // first attach — the link-judged entry (A5 seam evidence)
            __torajs_closure_props_attach(cell.cast::<u8>(), props);
        }
        props
    }
}

/// A function value's re-parent: the same-value short-circuit asks
/// the receiver's own [[GetPrototypeOf]] (its implicit chain is
/// %Function.prototype%, which the carrier's absent-entry read would
/// misreport as %Object.prototype% and refuse on a non-extensible
/// receiver), then the ordinary write through the expando carrier
/// with the CLOSURE's [[Extensible]] bit.
unsafe fn closure_set_prototype_of(
    obj: u64,
    cell: *mut c_void,
    proto: u64,
) -> Result<(), ProtoRefusal> {
    let proto_n = if is_null_any(proto) {
        VALUE_NULL_IMM
    } else {
        proto
    };
    if unsafe { crate::reflect_proto::proto_of_any_is(obj, proto_n) } {
        return Ok(());
    }
    let carrier = unsafe { closure_proto_carrier(cell) };
    unsafe { link_carrier(carrier, proto, cell_flags(cell)) }
}

/// [`link_proto`] with its own `PROTO_SLOT_KEY` Str.
unsafe fn link_carrier(carrier: *mut c_void, proto: u64, flags: u16) -> Result<(), ProtoRefusal> {
    unsafe {
        let key = alloc_str_key(PROTO_SLOT_KEY);
        let r = link_proto(carrier, proto, flags, key);
        __torajs_str_drop(key);
        r
    }
}

/// A struct instance's re-parent (RFC 20260920-struct-user-proto) —
/// the function-value path one tag over: the same-value test asks the
/// instance's own [[GetPrototypeOf]] (its class prototype until now),
/// the link lands in its lazy props dict, and `FLAG_OBJ_USER_PROTO`
/// tells every proto reader to look there instead of at the class tag.
unsafe fn struct_set_prototype_of(
    obj: u64,
    cell: *mut c_void,
    proto: u64,
) -> Result<(), ProtoRefusal> {
    let proto_n = if is_null_any(proto) {
        VALUE_NULL_IMM
    } else {
        proto
    };
    if unsafe { crate::reflect_proto::proto_of_any_is(obj, proto_n) } {
        return Ok(());
    }
    unsafe {
        let slot = cell.cast::<u8>().add(OBJ_PROPS_OFF) as *mut *mut c_void;
        if (*slot).is_null() {
            // first attach — the entry the instance's drop seams are
            // link-judged on
            __torajs_obj_props_attach(cell.cast::<u8>(), __torajs_dynobj_alloc());
        }
        link_carrier(*slot, proto, cell_flags(cell))?;
        crate::classmeta::obj_proto::__torajs_struct_user_proto_raise(cell);
    }
    Ok(())
}

/// The re-parent for a receiver tr keeps on its builtin
/// [[Prototype]] (an array, a wrapper, a Map, …): a same-value write
/// succeeds; a non-extensible receiver refuses a different one
/// (§10.1.2.1 step 5 — before this `Object.setPrototypeOf(frozenArr,
/// null)` returned quietly); an extensible one keeps its builtin
/// chain — the recorded boundary. Answers the step-5 refusal, or
/// `Ok` for both of the other outcomes.
unsafe fn exotic_set_prototype_of(obj: u64, proto: u64) -> Result<(), ProtoRefusal> {
    let proto = if is_null_any(proto) {
        VALUE_NULL_IMM
    } else {
        proto
    };
    if unsafe { crate::reflect_proto::proto_of_any_is(obj, proto) } {
        return Ok(());
    }
    if unsafe { cell_flags(obj as *const c_void) } & HDR_FLAG_NON_EXTENSIBLE != 0 {
        return Err(ProtoRefusal::NonExtensible);
    }
    Ok(())
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
        // §20.1.2.22 / §28.1.13 step 2 and §B.2.2.1.2 step 3 — "proto
        // is not an Object and proto is not null". `is_cell_imm`
        // answers a narrower question (is it a heap cell), and tr
        // carries strings, symbols and bigints on the heap while the
        // spec calls them primitives, so all three walked through a
        // gate that meant Object. The three faces differ in what they
        // do about it — two throw, `__proto__ =` returns — but not in
        // what they ask.
        if !is_null_any(proto) && !crate::reflect::is_object_imm(proto) {
            __torajs_throw_type_error(c"Prototype must be an object or null".as_ptr());
            return;
        }
        if !is_cell_imm(obj) {
            return;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) == crate::reflect::TAG_PROXY {
            // §10.5.2 — a Proxy answers the re-parent itself; a
            // refusal is §20.1.2.21 step 4's TypeError.
            if __torajs_proxy_set_prototype_of(obj, proto) == 0 && __torajs_throw_check() == 0 {
                __torajs_throw_type_error(c"proxy 'setPrototypeOf' trap returned falsish".as_ptr());
            }
            return;
        }
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // 405-01 substrate — a FUNCTION value re-parents through
            // its expando props bag (the extends lane's
            // `Object.setPrototypeOf(D, P)` static face).
            if heap_type_tag(cell) == TAG_CLOSURE {
                if let Err(r) = closure_set_prototype_of(obj, cell, proto) {
                    throw_proto_refusal(r);
                }
                return;
            }
            if heap_type_tag(cell) == TAG_OBJ {
                if let Err(r) = struct_set_prototype_of(obj, cell, proto) {
                    throw_proto_refusal(r);
                }
                return;
            }
            if let Err(r) = exotic_set_prototype_of(obj, proto) {
                throw_proto_refusal(r);
            }
            return;
        }
        if let Err(r) = ordinary_set_prototype_of(cell, proto, cell_flags(cell)) {
            throw_proto_refusal(r);
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
        // §20.1.2.22 / §28.1.13 step 2 and §B.2.2.1.2 step 3 — "proto
        // is not an Object and proto is not null". `is_cell_imm`
        // answers a narrower question (is it a heap cell), and tr
        // carries strings, symbols and bigints on the heap while the
        // spec calls them primitives, so all three walked through a
        // gate that meant Object. The three faces differ in what they
        // do about it — two throw, `__proto__ =` returns — but not in
        // what they ask.
        if !is_null_any(proto) && !crate::reflect::is_object_imm(proto) {
            __torajs_throw_type_error(c"Prototype must be an object or null".as_ptr());
            return 0;
        }
        if !is_cell_imm(obj) {
            return 0;
        }
        let cell = obj as *mut c_void;
        if heap_type_tag(cell) == crate::reflect::TAG_PROXY {
            return __torajs_proxy_set_prototype_of(obj, proto);
        }
        if heap_type_tag(cell) != TAG_DYNOBJ {
            // 405-01 substrate — function values re-parent through
            // the expando carrier, same as the Object flavor.
            if heap_type_tag(cell) == TAG_CLOSURE {
                return i64::from(closure_set_prototype_of(obj, cell, proto).is_ok());
            }
            if heap_type_tag(cell) == TAG_OBJ {
                return i64::from(struct_set_prototype_of(obj, cell, proto).is_ok());
            }
            // Exotic (Arr / wrapper) receivers cannot take a new
            // [[Prototype]] in tr — the
            // Object flavor throws / silently keeps; the honest
            // Reflect spelling of "the write cannot take effect" is
            // false (recorded boundary, same family as the
            // rotation-154 struct probe). A SAME-VALUE write takes
            // effect trivially (§10.1.2.1 step 4 answers true before
            // anything is asked of the receiver).
            let want = if is_null_any(proto) {
                VALUE_NULL_IMM
            } else {
                proto
            };
            return i64::from(crate::reflect_proto::proto_of_any_is(obj, want));
        }
        i64::from(ordinary_set_prototype_of(cell, proto, cell_flags(cell)).is_ok())
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
        // §20.1.2.22 / §28.1.13 step 2 and §B.2.2.1.2 step 3 — "proto
        // is not an Object and proto is not null". `is_cell_imm`
        // answers a narrower question (is it a heap cell), and tr
        // carries strings, symbols and bigints on the heap while the
        // spec calls them primitives, so all three walked through a
        // gate that meant Object. The three faces differ in what they
        // do about it — two throw, `__proto__ =` returns — but not in
        // what they ask.
        if !is_null_any(proto) && !crate::reflect::is_object_imm(proto) {
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
                if let Err(r) = closure_set_prototype_of(obj, cell, proto) {
                    throw_proto_refusal(r);
                }
                return;
            }
            if heap_type_tag(cell) == TAG_OBJ {
                if let Err(r) = struct_set_prototype_of(obj, cell, proto) {
                    throw_proto_refusal(r);
                }
                return;
            }
            if let Err(r) = exotic_set_prototype_of(obj, proto) {
                throw_proto_refusal(r);
            }
            return;
        }
        if let Err(r) = ordinary_set_prototype_of(cell, proto, cell_flags(cell)) {
            throw_proto_refusal(r);
        }
    }
}
