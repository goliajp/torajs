//! `Function.prototype.bind` (chunk 714) — mints a bound-function
//! cell off any callable that travels the `any` world.
//!
//! ES semantics projected:
//!
//! - Binding a reified builtin method (`s.toUpperCase.bind(s)`)
//!   captures the thisArg as the future RECEIVER — calling the
//!   bound function re-dispatches the original method id on it.
//! - Binding a plain closure captures nothing but the partial
//!   arguments (a torajs closure body cannot reference `this`).
//! - Binding a bound function nests: the outer thisArg is ignored
//!   (a bound this cannot be re-bound, ES §20.2.3.2) and the
//!   partial argument lists concatenate outside-in.
//! - The bound function is an ordinary callable — `typeof` answers
//!   "function", it rides HOF callbacks and `.call` / `.apply`
//!   (whose thisArg is likewise ignored), and expandos work.
//!
//! Cell layout — a closure-env shape (every callable probe keeps
//! working) with the bound state in the capture region:
//!
//!   offset  0 — universal header (rc=1, Tag::Closure, flags=0)
//!   offset  8 — fn_addr = the throwing native entry (method_value)
//!   offset 16 — drop_fn = [`bound_drop`]
//!   offset 24 — props (lazy expando bag, NULL)
//!   offset 32 — boxed_entry = [`bound_entry`]
//!   offset 40 — trace_fn = [`bound_trace`] (RFC 20260717
//!               closure-env-cycle — enumerates the held
//!               target/this/args children for the cycle collector)
//!   offset 48 — kind: 0 = builtin method id / 1 = closure-shaped
//!               target cell (held +1)
//!   offset 56 — target (method id or cell pointer)
//!   offset 64 — bound this (AnyValue, held ref on cells)
//!   offset 72 — bound argc
//!   offset 80+ — bound args (AnyValue each, held ref on cells)
//!
//! Unlike the interned method cells this cell is OWNED — rc-managed,
//! dropped through `drop_fn` like any synthesized closure env.

use core::ffi::c_void;

use torajs_rc::Tag;

use crate::method_call::{MAX_BOXED_ARGS, closure_cell_entry, invoke_with_this, not_callable};
use crate::method_value::{builtin_method_family, builtin_method_mid, native_entry};
use crate::nanbox::{AnyValue, VALUE_UNDEFINED};
use crate::nanbox_encode::__torajs_anyv_box_pointer;

unsafe extern "C" {
    /// torajs-rc / torajs-value-drop — NaN-box-safe release + the
    /// universal heap dropper (props bag, held target cell).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-cycle — scrub a normal-dropping cell from the cycle
    /// root buffer (RFC 20260717 knife 3; no-op when never buffered).
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
const CLOSURE_TRACE_FN_OFF: usize = 40;
const BOUND_KIND_OFF: usize = 48;
const BOUND_TARGET_OFF: usize = 56;
const BOUND_THIS_OFF: usize = 64;
const BOUND_ARGC_OFF: usize = 72;
const BOUND_ARGS_OFF: usize = 80;

fn bound_layout(argc: usize) -> core::alloc::Layout {
    core::alloc::Layout::from_size_align(BOUND_ARGS_OFF + argc * 8, 8).unwrap()
}

/// The `bind` arm — `recv.bind(thisArg, ...partial)` where `recv`
/// is the closure-tagged cell being bound. Returns the fresh bound
/// cell as an owned box, or the not-callable TypeError.
///
/// # Safety
/// `ptr` is a valid `Tag::Closure` heap pointer; `argv` points at
/// `argc` BORROWED AnyValue slots.
pub(crate) unsafe fn bind_cell(ptr: *mut c_void, argv: *const u64, argc: i64) -> AnyValue {
    unsafe {
        // Target classification: a reified builtin method re-binds
        // its receiver; anything with a boxed dual entry (plain
        // closures, other bound cells) dispatches through it.
        let (kind, target) = if let Some(mid) = builtin_method_mid(ptr) {
            (0u64, pack_builtin_target(mid, builtin_method_family(ptr)))
        } else if closure_cell_entry(ptr).is_some() {
            torajs_rc::__torajs_rc_inc(ptr);
            (1u64, ptr as u64)
        } else {
            return not_callable();
        };
        let this_arg = if argc >= 1 { *argv } else { VALUE_UNDEFINED };
        let n = (argc - 1).max(0) as usize;

        let cell = std::alloc::alloc_zeroed(bound_layout(n));
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = bound_drop as *const () as u64;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bound_entry as *const () as u64;
        *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = bound_trace as *const () as u64;
        *(cell.add(BOUND_KIND_OFF) as *mut u64) = kind;
        *(cell.add(BOUND_TARGET_OFF) as *mut u64) = target;
        // The cell takes its own stake on the captured values
        // (NaN-box-safe: immediates no-op).
        crate::nanbox_ffi::__torajs_anyv_rc_inc(this_arg);
        *(cell.add(BOUND_THIS_OFF) as *mut u64) = this_arg;
        *(cell.add(BOUND_ARGC_OFF) as *mut u64) = n as u64;
        for i in 0..n {
            let av = *argv.add(1 + i);
            crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
            *(cell.add(BOUND_ARGS_OFF + i * 8) as *mut u64) = av;
        }
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// A kind-0 bound target packs `mid | (family + 1) << 32`, the same
/// encoding the interned method cell uses for its capture slot.
/// Carrying the family (rather than the bare mid) is what keeps a
/// bound cell's dispatch identical to the `.call` path's: the
/// family-generic gate — §21.1.3 thisNumberValue / §20.3.3
/// thisBooleanValue brand checks, the §22.1.3 ToString(this) lane —
/// is selected by family, so dropping it made
/// `Boolean.prototype.toString.bind({})()` answer silently instead of
/// throwing. It also keeps the `.length` reflection family-aware.
fn pack_builtin_target(mid: i64, fam: i64) -> u64 {
    (mid as u32 as u64) | (((fam + 1) as u32 as u64) << 32)
}

/// Inverse of [`pack_builtin_target`].
pub(crate) fn unpack_builtin_target(target: u64) -> (i64, i64) {
    ((target as u32) as i64, ((target >> 32) as i64) - 1)
}

/// Bound-cell metadata `(kind, target, bound_argc)` — `None` for
/// every other closure shape (discriminated by the boxed entry's
/// address, the `builtin_method_mid` pattern). Chunk 719 — the
/// `.name` / `.length` reflection reads key off this.
///
/// # Safety
/// `ptr` is a live `Tag::Closure` cell.
pub(crate) unsafe fn bound_cell_meta(ptr: *mut c_void) -> Option<(u64, u64, usize)> {
    unsafe {
        let cell = ptr.cast::<u8>();
        let entry = *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry != bound_entry as *const () as u64 {
            return None;
        }
        Some((
            *(cell.add(BOUND_KIND_OFF) as *const u64),
            *(cell.add(BOUND_TARGET_OFF) as *const u64),
            *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize,
        ))
    }
}

/// Raw view of a bound cell's partial-argument block — the construct
/// kernel's §10.4.1.2 concatenation reads the slots in place (they
/// stay the cell's own BORROWED references).
///
/// # Safety
/// `ptr` is a live bound cell (`bound_cell_meta` answered `Some`).
pub(crate) unsafe fn bound_args_ptr(ptr: *mut c_void) -> *const u64 {
    unsafe { (ptr as *const u8).add(BOUND_ARGS_OFF) as *const u64 }
}

/// Boxed dual entry of a bound cell — concatenate the partial
/// arguments with the call's own and dispatch the target.
unsafe extern "C" fn bound_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let cell = env.cast::<u8>();
        let kind = *(cell.add(BOUND_KIND_OFF) as *const u64);
        let target = *(cell.add(BOUND_TARGET_OFF) as *const u64);
        let bound_this = *(cell.add(BOUND_THIS_OFF) as *const u64);
        let bn = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        let cn = argc.max(0) as usize;
        let total = bn + cn;

        let dispatch = |buf: *const u64, n: i64| -> AnyValue {
            if kind == 0 {
                // Same target shape `.call` / `.apply` build, so the
                // family-generic gate runs identically on both paths.
                let (mid, fam) = unpack_builtin_target(target);
                crate::method_call_closure::dispatch(
                    &crate::method_call_closure::CallTarget::Builtin(mid, fam),
                    bound_this,
                    buf,
                    n,
                )
            } else {
                let Some((env2, entry2)) = closure_cell_entry(target as *mut c_void) else {
                    return not_callable();
                };
                // The bound this rides the receiver channel — a
                // recv-first target takes it in argv[0] (RFC
                // 20260717-objlit-anylane-recv knife 2d), a plain
                // closure drops it.
                invoke_with_this(env2, entry2, bound_this, buf, n)
            }
        };

        // Argument slots stay BORROWED end to end — the bound args
        // are the cell's own references, the tail is the caller's.
        if total > MAX_BOXED_ARGS {
            let mut big = vec![VALUE_UNDEFINED; total];
            for (i, slot) in big.iter_mut().enumerate().take(bn) {
                *slot = *(cell.add(BOUND_ARGS_OFF + i * 8) as *const u64);
            }
            for i in 0..cn {
                big[bn + i] = *argv.add(i);
            }
            return dispatch(big.as_ptr(), total as i64);
        }
        let mut buf = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        for (i, slot) in buf.iter_mut().enumerate().take(bn) {
            *slot = *(cell.add(BOUND_ARGS_OFF + i * 8) as *const u64);
        }
        for i in 0..cn.min(MAX_BOXED_ARGS - bn) {
            buf[bn + i] = *argv.add(i);
        }
        dispatch(buf.as_ptr(), total.min(MAX_BOXED_ARGS) as i64)
    }
}

/// drop_fn of a bound cell — the universal Closure drop arm calls
/// this on hit-zero; release every captured reference, the lazy
/// props bag, then the block itself.
/// The bound cell's trace fn (RFC 20260717 closure-env-cycle knife
/// 2) — enumerates the same held children [`bound_drop`] releases,
/// minus the +24 props bag (fixed shared offset; the collector's
/// Closure arm reads it directly). Children are raw slot payloads —
/// AnyValue bits for this/args — and the collector's visit callback
/// owns the cell-like gate, mirroring the SSA-emitted
/// `__env_trace_<fn>` contract.
unsafe extern "C" fn bound_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let cell = env.cast::<u8>();
        if *(cell.add(BOUND_KIND_OFF) as *const u64) == 1 {
            let slot = cell.add(BOUND_TARGET_OFF);
            visit(
                0,
                *(slot as *const u64) as *mut c_void,
                slot as *mut c_void,
                ctx,
            );
        }
        let this_slot = cell.add(BOUND_THIS_OFF);
        visit(
            1,
            *(this_slot as *const u64) as *mut c_void,
            this_slot as *mut c_void,
            ctx,
        );
        let bn = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        for i in 0..bn {
            let slot = cell.add(BOUND_ARGS_OFF + i * 8);
            visit(
                2 + i as i64,
                *(slot as *const u64) as *mut c_void,
                slot as *mut c_void,
                ctx,
            );
        }
    }
}

unsafe extern "C" fn bound_drop(env: *mut c_void) {
    unsafe {
        // Scrub the cycle root buffer first — a buffered bound cell
        // that normal-drops here would leave a dangling entry (RFC
        // 20260717 knife 3; cheap no-op when FLAG_BUFFERED is clear).
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let this_v = *(cell.add(BOUND_THIS_OFF) as *const u64);
        crate::nanbox_ffi::__torajs_anyv_rc_dec(this_v);
        let bn = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        for i in 0..bn {
            crate::nanbox_ffi::__torajs_anyv_rc_dec(
                *(cell.add(BOUND_ARGS_OFF + i * 8) as *const u64),
            );
        }
        if *(cell.add(BOUND_KIND_OFF) as *const u64) == 1 {
            let target = *(cell.add(BOUND_TARGET_OFF) as *const u64);
            __torajs_value_drop_heap(target as *mut c_void);
        }
        std::alloc::dealloc(cell, bound_layout(bn));
    }
}
