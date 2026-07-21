//! `Promise.withResolvers()` kernel (§27.2.4.8, ES2024) — mints a
//! pending promise plus its two settle functions and answers the
//! spec result object `{ promise, resolve, reject }` as a boxed
//! dynobj.
//!
//! The resolve / reject values are per-instance rc-managed closure
//! cells (the [`crate::method_bind`] owned-cell shape, one capture):
//!
//! ```text
//!   offset  0 — universal header (rc=1, Tag::Closure, flags=0)
//!   offset  8 — fn_addr = the throwing native entry (method_value)
//!   offset 16 — drop_fn = [`resolver_drop`]
//!   offset 24 — props — pre-seeded with the §20.2.3 own `name` ""
//!               / `length` 1 reflection entries (W0/E0/C1, the
//!               %GeneratorPrototype% step-method precedent in
//!               [`crate::closure_proto`])
//!   offset 32 — boxed_entry = resolve / reject settle entry
//!   offset 40 — trace_fn = [`resolver_trace`] (the held promise is
//!               a cycle-collector child)
//!   offset 48 — promise cell (held +1)
//! ```
//!
//! Settle semantics: first settle wins — an already-settled promise
//! makes the call a silent no-op (no stake taken). The stored value
//! is the caller's argv[0] AnyValue verbatim under `REPR_ANY` /
//! `value_is_heap=1` (the cell is pre-stamped at mint so the
//! any-lane `.then` bridge accepts attaches before settlement).
//!
//! Recorded edge: `resolve(thenable)` stores the thenable itself
//! instead of adopting its state (§27.2.1.3.2 PromiseResolveFunctions
//! step 9 adoption) — the adoption wire is a plan-state follow-up.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::nanbox::VALUE_UNDEFINED;
use crate::nanbox_encode::__torajs_anyv_box_pointer;

// ---- torajs-promise layout mirror (lockstep layout.rs) ----
const P_STATE_OFF: usize = 8;
const P_IS_HEAP_OFF: usize = 9;
const P_REPR_OFF: usize = 11;
const STATE_PENDING: u8 = 0;
const REPR_ANY: u8 = 6;

// ---- closure cell layout (ssa_lower closure-env mirror) ----
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_TRACE_FN_OFF: usize = 40;
const RESOLVER_PROMISE_OFF: usize = 48;
const CELL_SIZE: usize = 56;

/// dynobj bucket tags (torajs-dynobj `layout.rs` mirror).
const ANY_HEAP: u64 = 4;
const ANY_I64: u64 = 2;

/// Reflection entry flags — value present + all three present,
/// writable false / enumerable false / configurable true (mirror of
/// `closure_proto::REFLECT_ENTRY_FLAGS`; §20.2.3 fn name/length).
const REFLECT_ENTRY_FLAGS: u64 = (1 << 6) | (1 << 5) | (1 << 4) | (1 << 3) | (1 << 2);

unsafe extern "C" {
    fn __torajs_promise_alloc_pending() -> *mut c_void;
    fn __torajs_promise_resolve(p: *mut c_void, value: i64);
    fn __torajs_promise_reject(p: *mut c_void, reason: i64);
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
    fn __torajs_dynobj_define(
        obj_slot: *mut *mut c_void,
        key: *mut c_void,
        tag: u64,
        value: u64,
        flags_byte: u64,
    );
    fn __torajs_str_alloc(bytes: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — scrub a normal-dropping cell from the cycle
    /// root buffer (no-op when never buffered).
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// `Promise.withResolvers()` — the ssa_lower `Promise.<static>` arm
/// calls this 0-arg kernel; the return is the boxed result dynobj
/// (owned, transfers to the caller).
///
/// # Safety
/// FFI face; allocates and wires fresh cells only.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_promise_with_resolvers() -> u64 {
    unsafe {
        let p = __torajs_promise_alloc_pending();
        // Pre-stamp the any-lane value form — attaches arrive before
        // settlement and the bridge's attach-time gate must see a
        // known repr (the method_call_promise stamp_result pattern).
        p.cast::<u8>().add(P_REPR_OFF).write(REPR_ANY);
        p.cast::<u8>().add(P_IS_HEAP_OFF).write(1);
        // Each resolver holds its own +1 on the promise; the alloc's
        // original ref transfers into the result object's `promise`
        // slot below.
        torajs_rc::__torajs_rc_inc(p);
        let r = mint_resolver(p, resolver_resolve_entry);
        torajs_rc::__torajs_rc_inc(p);
        let j = mint_resolver(p, resolver_reject_entry);

        let mut obj = __torajs_dynobj_alloc();
        set_field(&mut obj, b"promise", p as u64);
        set_field(&mut obj, b"resolve", r as u64);
        set_field(&mut obj, b"reject", j as u64);
        __torajs_anyv_box_pointer(obj)
    }
}

/// Ordinary W1/E1/C1 data entry on the result object — the value is
/// a fresh heap cell whose ref TRANSFERS into the bucket; the key is
/// borrowed (the store incs its own on insert).
unsafe fn set_field(obj_slot: *mut *mut c_void, name: &[u8], value: u64) {
    unsafe {
        let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
        __torajs_dynobj_set(obj_slot, key as *mut c_void, ANY_HEAP, value);
        __torajs_str_drop(key as *mut c_void);
    }
}

/// One settle-function cell — owned, carries the promise at the
/// capture slot, props pre-seeded with the spec reflection entries
/// (`name` "" / `length` 1).
unsafe fn mint_resolver(
    p: *mut c_void,
    entry: unsafe extern "C" fn(*mut c_void, *const u64, i64) -> u64,
) -> *mut u8 {
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) =
            crate::method_value::native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = resolver_drop as *const () as u64;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = entry as *const () as u64;
        *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = resolver_trace as *const () as u64;
        *(cell.add(RESOLVER_PROMISE_OFF) as *mut u64) = p as u64;

        let props_slot = cell.add(CLOSURE_PROPS_OFF) as *mut *mut c_void;
        *props_slot = __torajs_dynobj_alloc();
        let name_key = __torajs_str_alloc(c"name".as_ptr() as *const u8, 4);
        let empty = __torajs_str_alloc(c"".as_ptr() as *const u8, 0);
        __torajs_dynobj_define(
            props_slot,
            name_key as *mut c_void,
            ANY_HEAP,
            empty as u64,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(name_key as *mut c_void);
        let len_key = __torajs_str_alloc(c"length".as_ptr() as *const u8, 6);
        __torajs_dynobj_define(
            props_slot,
            len_key as *mut c_void,
            ANY_I64,
            1,
            REFLECT_ENTRY_FLAGS,
        );
        __torajs_str_drop(len_key as *mut c_void);
        cell
    }
}

/// Shared settle body — first settle wins; the stored value takes
/// its own stake off the BORROWED argv slot (NaN-box-aware inc,
/// immediates no-op).
unsafe fn settle(env: *mut c_void, argv: *const u64, argc: i64, rejected: bool) -> u64 {
    unsafe {
        let p = *(env.cast::<u8>().add(RESOLVER_PROMISE_OFF) as *const u64) as *mut c_void;
        if p.cast::<u8>().add(P_STATE_OFF).read() != STATE_PENDING {
            return VALUE_UNDEFINED;
        }
        let v = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        crate::nanbox_ffi::__torajs_anyv_rc_inc(v);
        if rejected {
            __torajs_promise_reject(p, v as i64);
        } else {
            __torajs_promise_resolve(p, v as i64);
        }
        VALUE_UNDEFINED
    }
}

unsafe extern "C" fn resolver_resolve_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { settle(env, argv, argc, false) }
}

unsafe extern "C" fn resolver_reject_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe { settle(env, argv, argc, true) }
}

/// drop_fn — release the props bag + the held promise, then the
/// block itself (the [`crate::method_bind::bound_drop`] shape).
unsafe extern "C" fn resolver_drop(env: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let p = *(cell.add(RESOLVER_PROMISE_OFF) as *const u64);
        if p != 0 {
            __torajs_value_drop_heap(p as *mut c_void);
        }
        std::alloc::dealloc(
            cell,
            core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap(),
        );
    }
}

/// trace_fn — the held promise is the cell's one extra child (the
/// +24 props bag rides the collector's fixed Closure-arm read).
unsafe extern "C" fn resolver_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let slot = env.cast::<u8>().add(RESOLVER_PROMISE_OFF);
        visit(
            0,
            *(slot as *const u64) as *mut c_void,
            slot as *mut c_void,
            ctx,
        );
    }
}
