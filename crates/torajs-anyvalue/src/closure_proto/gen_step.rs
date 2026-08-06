//! `%GeneratorPrototype%` / `%AsyncGeneratorPrototype%` step methods
//! — the `next` / `return` / `throw` cells torajs-meta's genfn mint
//! defines into each kind's shared prototype, and the call face they
//! answer when invoked through a DETACHED reference
//! (`GP.next.call(g)`) rather than through the instance.
//!
//! Split out of the parent (rotation 320) because the parent answers
//! a different question — how a user function's own `.prototype` /
//! `.constructor` faces materialize — and this family had grown to
//! most of the file. A child module rather than a sibling so the
//! parent's private externs, `interned_key` and `ANY_HEAP` stay
//! reachable with zero visibility changes.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use super::{
    __torajs_anyv_get_proto_of_any, __torajs_dynobj_alloc, __torajs_dynobj_define,
    __torajs_promise_alloc_rejected_heap, __torajs_promise_stamp_repr, __torajs_throw_set,
    __torajs_throw_take, __torajs_throw_take_tag, __torajs_throw_type_error, ANY_HEAP,
    interned_key,
};
use crate::method_value::mint_immortal_str;

/// `%GeneratorPrototype%` step-method cells (RFC 20260721 刀 2) —
/// `next` / `return` / `throw` reified for the REFLECTION surface
/// (typeof / name / length / gOPD): interned per (kind, which),
/// `name` / `length` carried as own W0/E0/C1 props entries so the
/// existing closure member/gOPD chains answer them with zero new
/// plumbing. A detached CALL raises the recorded loud TypeError —
/// live stepping rides the generator instance's class methods, and
/// a receiver-generic re-dispatch is the recorded face.
static GEN_STEP_CELLS: [[AtomicU64; 3]; 2] = [
    [const { AtomicU64::new(0) }; 3],
    [const { AtomicU64::new(0) }; 3],
];
static GEN_STEP_NAMES: [&[u8]; 3] = [b"next", b"return", b"throw"];

unsafe extern "C" fn gen_step_reject_entry(
    _env: *mut c_void,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"generator prototype step method called through a detached value is not supported"
                .as_ptr(),
        );
    }
    crate::nanbox::VALUE_UNDEFINED
}

/// The shared `%GeneratorPrototype%` (kind 0) /
/// `%AsyncGeneratorPrototype%` (kind 1) object, handed over by
/// torajs-meta's genfn mint as it wires the step cells in. Borrowed
/// pointers into immortal mint state — never released, only compared.
static GEN_PROTO_CELLS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// genfn-mint face — record the shared step-method prototype of
/// `kind` so [`receiver_is_gen_instance`] has a chain root to match.
///
/// # Safety
/// FFI face; `proto` is the mint-owned dynobj (or null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_gen_proto_register(kind: i64, proto: *mut c_void) {
    let k = if kind == 1 { 1usize } else { 0usize };
    GEN_PROTO_CELLS[k].store(proto as u64, Ordering::Relaxed);
}

/// Chain-walk bound — an instance sits exactly two hops under its
/// kind's shared prototype; the cap only stops a cyclic user chain.
const GEN_CHAIN_MAX_HOPS: usize = 32;

/// Does `recv` carry the `[[AsyncGeneratorState]]` slot §27.6.1.2
/// step 3 tests for (resp. `[[GeneratorState]]` for kind 0)?
///
/// tr keeps no internal-slot table, so the structural proxy is the
/// prototype chain. A `__Gen_<name>` INSTANCE reaches the shared
/// prototype in two hops (instance → its class proto → the shared
/// proto); the class proto itself (`g.prototype`) reaches it in one,
/// and the shared proto in zero. Requiring depth ≥ 2 therefore
/// accepts exactly the instances and rejects `g`, `g.prototype`,
/// `%AsyncGeneratorPrototype%` itself, sync-generator instances
/// (a different kind's root) and every non-generator value.
///
/// Boundary: a hand-built `Object.create(g.prototype)` has no state
/// slot yet answers `true`. That direction is deliberate — it keeps
/// the loud TypeError of the not-yet-implemented detached-step face
/// instead of quietly handing back a rejected promise that reads as
/// "this was never a generator".
unsafe fn receiver_is_gen_instance(recv: u64, kind: usize) -> bool {
    let target = GEN_PROTO_CELLS[kind].load(Ordering::Relaxed);
    if target == 0 || !crate::nanbox::is_cell(recv) {
        return false;
    }
    unsafe {
        let mut cur = __torajs_anyv_get_proto_of_any(recv);
        let mut depth = 1usize;
        while crate::nanbox::is_cell(cur) && depth <= GEN_CHAIN_MAX_HOPS {
            let hit = crate::nanbox::as_void_ptr(cur) as u64 == target;
            let next = if hit {
                0
            } else {
                __torajs_anyv_get_proto_of_any(cur)
            };
            crate::nanbox_ffi::__torajs_anyv_rc_dec(cur);
            if hit {
                return depth >= 2;
            }
            cur = next;
            depth += 1;
        }
    }
    false
}

/// A fresh already-rejected promise carrying a real TypeError
/// instance — §27.6.1.2-4 route a bad receiver through
/// AsyncGeneratorEnqueue step 3 (reject the capability's promise)
/// instead of throwing, so the caller's `.then` observes it.
///
/// The instance comes from the same native-error factory registry
/// that backs every runtime TypeError: raise it, then take it back
/// out of the pending-throw slot so it can settle the promise rather
/// than unwind. Taking also clears the active flag, which this path
/// requires — it returns a value, so the caller's `emit_throw_check`
/// must find a clean slot.
unsafe fn rejected_type_error_promise() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"AsyncGenerator.prototype step method called on a receiver that is not an async generator"
                .as_ptr(),
        );
        let tag = __torajs_throw_take_tag();
        let reason = __torajs_throw_take();
        // No factory registered (no Error class reachable in this
        // program) leaves a non-heap slot: there is nothing sound to
        // reject WITH, so re-arm and stay loud.
        if tag != ANY_HEAP as i64 || reason == 0 {
            __torajs_throw_set(tag, reason);
            return crate::nanbox::VALUE_UNDEFINED;
        }
        let p = __torajs_promise_alloc_rejected_heap(reason);
        if p.is_null() {
            __torajs_throw_set(tag, reason);
            return crate::nanbox::VALUE_UNDEFINED;
        }
        __torajs_promise_stamp_repr(p, crate::promise_with_resolvers::REPR_ANY as i64);
        crate::nanbox::box_void_ptr(p)
    }
}

/// The async trio's call face (§27.6.1.2-4). Unlike the sync kind,
/// a bad receiver REJECTS rather than throws, so this entry has to
/// tell a genuine async generator from everything else.
unsafe extern "C" fn async_gen_step_reject_entry(
    _env: *mut c_void,
    argv: *const u64,
    argc: i64,
) -> u64 {
    // FLAG_CLOSURE_RECV_FIRST puts the call-site `this` in slot 0; a
    // bare `AGP.next()` lands `undefined` there, which is exactly the
    // non-object receiver step 3 rejects.
    let recv = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        crate::nanbox::VALUE_UNDEFINED
    };
    if unsafe { receiver_is_gen_instance(recv, 1) } {
        // A real async generator: stepping it through the detached
        // face needs the receiver-generic re-dispatch tr does not
        // have yet. Keep the loud TypeError — answering a rejected
        // promise here would misreport a live generator as an
        // incompatible receiver.
        unsafe {
            __torajs_throw_type_error(
                c"generator prototype step method called through a detached value is not supported"
                    .as_ptr(),
            );
        }
        return crate::nanbox::VALUE_UNDEFINED;
    }
    unsafe { rejected_type_error_promise() }
}

/// The interned `%GeneratorPrototype%.next/return/throw` cell —
/// torajs-meta's genfn trio mint defines these into the shared
/// gen_proto. `kind` 0 = generator / 1 = async generator; `which`
/// indexes [`GEN_STEP_NAMES`]. Answers an immortal cell (borrow).
///
/// # Safety
/// FFI face; indices are clamped by the callers (genfn mint).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_gen_step_method_cell(kind: i64, which: i64) -> *mut u8 {
    let k = if kind == 1 { 1usize } else { 0usize };
    let w = (which as usize).min(2);
    let slot = &GEN_STEP_CELLS[k][w];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // §27.5.1.2-4 THROW on a bad receiver; §27.6.1.2-4 REJECT (the
    // async trio routes through AsyncGeneratorEnqueue), so the two
    // kinds carry different call faces.
    let cell = if k == 1 {
        crate::method_value::mint_reject_closure_cell(async_gen_step_reject_entry)
    } else {
        crate::method_value::mint_reject_closure_cell(gen_step_reject_entry)
    };
    unsafe {
        if k == 1 {
            // The async face must SEE the receiver to separate a live
            // generator from the values step 3 rejects; bit 12 makes
            // the dispatcher pass the call-site `this` in argv[0].
            *(cell.add(6) as *mut u16) |= torajs_rc::FLAG_CLOSURE_RECV_FIRST;
        }
        // name / length as own props entries ({W:0, E:0, C:1},
        // §27.5.1.2-4: every step method's length is 1).
        let props_slot = cell.add(24) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_cell = mint_immortal_str(GEN_STEP_NAMES[w]);
        __torajs_dynobj_define(
            props_slot,
            interned_key(&NAME_KEY_CELL, b"name"),
            ANY_HEAP,
            name_cell as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_dynobj_define(
            props_slot,
            interned_key(&LENGTH_KEY_CELL, b"length"),
            ANY_I64,
            1,
            REFLECT_ENTRY_FLAGS,
        );
    }
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

/// `ANY_I64` slot tag (torajs-dynobj `layout.rs` mirror).
const ANY_I64: u64 = 2;
/// Reflection entry: value present + all three present, writable
/// false / enumerable false / configurable true.
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

static NAME_KEY_CELL: AtomicU64 = AtomicU64::new(0);
static LENGTH_KEY_CELL: AtomicU64 = AtomicU64::new(0);
