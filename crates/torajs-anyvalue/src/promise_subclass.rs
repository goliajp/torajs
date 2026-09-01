//! Promise-subclass instance mint + ctor `super(executor)` (RFC
//! 20260730-exotic-backed-class-instance blade 2).
//!
//! `class C extends Promise` mints a REAL pending Promise cell — the
//! whole then/catch/await machinery rides the existing arms because
//! the instance IS a promise. Class identity rides blade 0
//! (`FLAG_SUBCLASSED` + side table, scrubbed by the promise drop
//! path).
//!
//! `super(executor)` is §27.2.3.1: reject a non-callable executor
//! (step 2's TypeError), mint the §27.2.1.3 resolving-function pair
//! bound to THIS instance (the withResolvers cells, reused), run the
//! executor synchronously against them, and convert an executor
//! throw into a rejection (step 10 — respecting
//! `[[AlreadyResolved]]`: a throw after `resolve()` ran is
//! swallowed, first settle already won). Unlike the other exotic
//! parents there is NO zero-argument no-op form — `super()` routes
//! here with undefined and takes the TypeError.

use core::ffi::c_void;

use torajs_rc::FLAG_SUBCLASSED;

use crate::method_call_closure_dispatch::invoke_boxed;
use crate::nanbox::{as_void_ptr, is_cell};
use crate::promise_with_resolvers::{
    P_IS_HEAP_OFF, P_REPR_OFF, P_RESOLVED_LOCK_OFF, P_STATE_OFF, REPR_ANY, STATE_PENDING,
    mint_resolver, resolver_reject_entry, resolver_resolve_entry,
};

/// `torajs_rc::AnySlotTag::Heap` mirror.
const ANY_HEAP: i64 = 4;

unsafe extern "C" {
    /// torajs-meta — blade-0 identity registration / classmeta proto.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// torajs-promise — fresh pending cell.
    fn __torajs_promise_alloc_pending() -> *mut c_void;
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    /// torajs-anyvalue nanbox FFI (same-crate extern face keeps the
    /// pair-boxing route the other subclass kernels use).
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// torajs-throw — pending-throw probe / consume (the executor
    /// ran user code; a raise must become this promise's rejection).
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_take() -> i64;
    fn __torajs_throw_take_tag() -> i64;
    /// torajs-throw — §27.2.3.1 step 2.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// Universal drop dispatcher — release the resolver temps.
    fn __torajs_value_drop_heap(child: *mut c_void);
}

/// Mint a Promise-subclass instance: a fresh PENDING promise cell,
/// pre-stamped for the any-lane value form (the withResolvers stamp —
/// attaches may arrive before settlement), carrying the class
/// identity. Answers the boxed instance.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let p = __torajs_promise_alloc_pending();
        p.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
        p.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
        let flags = p.cast::<u8>().add(6) as *mut u16;
        flags.write(flags.read() | FLAG_SUBCLASSED);
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(p, class_tag, proto_cell);
        __torajs_anyv_box_from_pair(ANY_HEAP, p as i64)
    }
}

/// `super(executor)` inside a Promise-subclass ctor — see module doc.
/// Answers a fresh owned reference on the instance (statement-
/// position release convention); the TypeError path answers the
/// borrowed box un-inc'd for the throw-check to divert past.
///
/// # Safety
/// `this_av` is the factory's freshly minted subclass instance boxed
/// ANY_HEAP (or any non-cell box, answered back unchanged); `ex_av`
/// is any boxed value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_subclass_super(this_av: u64, ex_av: u64) -> u64 {
    unsafe {
        // Cell test on the box itself (546-02): the doc's "any
        // non-cell box answered back unchanged" arm used to run
        // through unbox_value, which materializes a ShortStr into an
        // owned Str this function then treated as the instance cell
        // (resolvers minted over a Str block). A cell's box IS the
        // pointer.
        if !is_cell(this_av) {
            return this_av;
        }
        let p = this_av as *mut c_void;
        // §27.2.3.1 step 2 — IsCallable(executor).
        let target = if is_cell(ex_av) {
            crate::method_call_closure_dispatch::closure_cell_entry(as_void_ptr(ex_av))
        } else {
            None
        };
        let Some((env, entry)) = target else {
            __torajs_throw_type_error(c"Promise executor must be a function".as_ptr());
            return this_av;
        };
        // §27.2.1.3 CreateResolvingFunctions over THIS instance —
        // each resolver holds its own +1 on the promise.
        torajs_rc::__torajs_rc_inc(p);
        let resolve = mint_resolver(p, resolver_resolve_entry);
        torajs_rc::__torajs_rc_inc(p);
        let reject = mint_resolver(p, resolver_reject_entry);
        let argv = [
            crate::nanbox_encode::__torajs_anyv_box_pointer(resolve as *mut c_void),
            crate::nanbox_encode::__torajs_anyv_box_pointer(reject as *mut c_void),
        ];
        let out = invoke_boxed(env, entry, argv.as_ptr(), 2);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(out);
        // §27.2.3.1 step 10 — an executor throw rejects, unless a
        // settle already won ([[AlreadyResolved]] gate, same as the
        // resolver bodies).
        if __torajs_throw_check() != 0 {
            let tag = __torajs_throw_take_tag();
            let value = __torajs_throw_take();
            let boxed = __torajs_anyv_box_from_pair(tag, value);
            if p.cast::<u8>().add(P_STATE_OFF).read() == STATE_PENDING
                && p.cast::<u8>().add(P_RESOLVED_LOCK_OFF).read() == 0
            {
                p.cast::<u8>().add(P_RESOLVED_LOCK_OFF).write(1);
                // The taken throw value is an owned transfer; the
                // reject consumes exactly one reference.
                __torajs_promise_reject(p, boxed as i64);
            } else {
                crate::nanbox_ffi::__torajs_anyv_rc_dec(boxed);
            }
        }
        // Release this call's own resolver references (an executor
        // that stored them took its own stakes).
        __torajs_value_drop_heap(resolve as *mut c_void);
        __torajs_value_drop_heap(reject as *mut c_void);
        torajs_rc::__torajs_rc_inc(p);
        this_av
    }
}
