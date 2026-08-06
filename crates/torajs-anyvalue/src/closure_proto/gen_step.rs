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
/// plumbing. A detached CALL re-dispatches a live instance onto its
/// own class methods and otherwise takes the kind's spec failure —
/// see [`gen_step_call`].
static GEN_STEP_CELLS: [[AtomicU64; 3]; 2] = [
    [const { AtomicU64::new(0) }; 3],
    [const { AtomicU64::new(0) }; 3],
];
static GEN_STEP_NAMES: [&[u8]; 3] = [b"next", b"return", b"throw"];

/// Interned name cells for the re-dispatch below — the dispatcher
/// resolves a class method by NAME, and `throw` has no builtin mid
/// at all, so the name is the part that always carries.
static GEN_STEP_NAME_CELLS: [AtomicU64; 3] = [const { AtomicU64::new(0) }; 3];

fn gen_step_name_cell(which: usize) -> *mut u8 {
    let slot = &GEN_STEP_NAME_CELLS[which];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    let cell = mint_immortal_str(GEN_STEP_NAMES[which]);
    slot.store(cell as u64, Ordering::Relaxed);
    cell
}

/// The builtin mid each step interns to. `throw` is absent from the
/// intern table, so it takes `ANY_METHOD_UNKNOWN` and resolves the
/// way any user method does — by name.
fn gen_step_mid(which: usize) -> i64 {
    match which {
        0 => torajs_rc::ANY_METHOD_NEXT,
        1 => torajs_rc::any_method::ANY_METHOD_ITER_RETURN,
        _ => torajs_rc::ANY_METHOD_UNKNOWN,
    }
}

/// The call face shared by both kinds (§27.5.1.2-4 sync /
/// §27.6.1.2-4 async).
///
/// `FLAG_CLOSURE_RECV_FIRST` puts the call-site `this` in slot 0 and
/// the real arguments after it, so a bare `GP.next()` lands
/// `undefined` there — exactly the non-object receiver step 3
/// rejects.
///
/// A genuine generator instance RE-DISPATCHES onto its own step
/// method. That is the whole of the detached face: `GP.next.call(g)`
/// and `g.next()` are required to be the same call, and the instance
/// already answers the latter through its `__Gen_<name>` class
/// methods. Resolution finds those first because the class proto
/// sits BELOW the shared prototype these cells live on — which is
/// also why re-dispatch cannot reach back into this entry and spin.
///
/// Everything else keeps the kind's spec-mandated failure, and the
/// two kinds differ: sync THROWS, async REJECTS (its trio routes
/// through AsyncGeneratorEnqueue). That split is why the receiver
/// test has to happen here rather than at the call site.
unsafe fn gen_step_call(kind: usize, which: usize, argv: *const u64, argc: i64) -> u64 {
    let recv = if argc > 0 && !argv.is_null() {
        unsafe { *argv }
    } else {
        crate::nanbox::VALUE_UNDEFINED
    };
    if unsafe { receiver_is_gen_instance(recv, kind) } {
        let (rest, rest_argc) = if argc > 1 {
            (unsafe { argv.add(1) }, argc - 1)
        } else {
            (core::ptr::null(), 0)
        };
        return unsafe {
            crate::method_call::any_method_call_inner(
                recv,
                gen_step_mid(which),
                gen_step_name_cell(which),
                core::ptr::null_mut(),
                rest,
                rest_argc,
            )
        };
    }
    if kind == 1 {
        return unsafe { rejected_type_error_promise() };
    }
    unsafe {
        __torajs_throw_type_error(
            c"Generator.prototype step method called on a receiver that is not a generator"
                .as_ptr(),
        );
    }
    crate::nanbox::VALUE_UNDEFINED
}

/// One boxed entry per (kind, which). The body needs to know which
/// step it is in order to re-dispatch, and a `mint_reject_closure_cell`
/// cell carries no capture channel to read it from — the cap slot
/// already encodes the builtin-method family.
macro_rules! step_entry {
    ($name:ident, $kind:expr, $which:expr) => {
        unsafe extern "C" fn $name(_env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
            unsafe { gen_step_call($kind, $which, argv, argc) }
        }
    };
}

step_entry!(sync_step_next, 0, 0);
step_entry!(sync_step_return, 0, 1);
step_entry!(sync_step_throw, 0, 2);
step_entry!(async_step_next, 1, 0);
step_entry!(async_step_return, 1, 1);
step_entry!(async_step_throw, 1, 2);

/// The (kind, which) call face a freshly minted cell installs.
fn step_entry_for(
    kind: usize,
    which: usize,
) -> unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64 {
    match (kind, which) {
        (0, 0) => sync_step_next,
        (0, 1) => sync_step_return,
        (0, _) => sync_step_throw,
        (_, 0) => async_step_next,
        (_, 1) => async_step_return,
        (_, _) => async_step_throw,
    }
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
/// slot yet answers `true`, so it re-dispatches onto the class
/// method and gets whatever that does with a stateless receiver.
/// That is the SAME answer `Object.create(g.prototype).next()` gives
/// directly, which is the point — the detached form is required to
/// be the direct form. The divergence from node (which reads the
/// missing slot and throws) lives in the class method, one level
/// down, and is a recorded boundary there rather than something for
/// this gate to paper over.
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
    // async trio routes through AsyncGeneratorEnqueue), and either
    // kind re-dispatches a live instance onto its own step method —
    // so the face is per (kind, which).
    let cell = crate::method_value::mint_reject_closure_cell(step_entry_for(k, w));
    unsafe {
        // Every face must SEE the receiver: to separate a live
        // generator from the values step 3 refuses, and to forward it
        // when re-dispatching. Bit 12 makes the dispatcher pass the
        // call-site `this` in argv[0] and the real arguments after.
        *(cell.add(6) as *mut u16) |= torajs_rc::FLAG_CLOSURE_RECV_FIRST;
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
