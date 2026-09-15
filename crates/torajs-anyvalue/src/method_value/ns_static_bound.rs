//! Self-hosted namespace statics (RFC 20260914-array-from-async-ts)
//! — a static whose implementation is a TypeScript function the
//! compiler injects into the program (`Array.fromAsync` today). The
//! cell's call face has nothing of its own to run: the program
//! registers its closure at startup through
//! [`__torajs_ns_static_bind`], and a call through the reified cell
//! (`const f = Array.fromAsync; f(…)`, `(Array as any).fromAsync(…)`,
//! `Reflect.apply(Array.fromAsync, …)`) forwards to it. The direct
//! spellings never come here — the injector rewrites them to the
//! helper by name — so the two routes share the one body.
//!
//! The bound closure takes the cell's recv-first argv shape as its
//! parameters (`this_c, items, mapfn, thisArg`); a caller that wrote
//! fewer arguments gets `undefined` in the rest, the way a declared
//! parameter does.

use core::sync::atomic::{AtomicU64, Ordering};

use super::TABLE_SIZE;
use super::ns_static_table::__torajs_throw_type_error;
use super::ns_static_util::{arg_at, own};
use crate::nanbox::VALUE_UNDEFINED;

/// One slot per ns-static id (the same row space as `NS_CELLS`);
/// 0 = the program bound nothing here.
static BOUND: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Parameters every self-hosted static is called with — the recv-first
/// cell's `(this, a, b, c)`.
const BOUND_ARITY: usize = 4;

/// `__torajs_ns_static_bind(id, f)` — the program's startup
/// registration. `f` is BORROWED (the lowering releases its own
/// stake after the call); the table takes one of its own and keeps
/// it for the program's lifetime. Binding twice releases the earlier
/// closure.
///
/// # Safety
/// `f` is a live any-boxed value for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_ns_static_bind(id: i64, f: u64) {
    let Some(slot) = usize::try_from(id).ok().and_then(|i| BOUND.get(i)) else {
        // The id is a compiler-baked constant off the shared table —
        // an out-of-range one is a compiler defect, not a runtime
        // condition; refuse loudly rather than write past the table.
        unsafe {
            __torajs_throw_type_error(
                c"namespace static bind: id outside the static table".as_ptr(),
            );
        }
        return;
    };
    let prev = slot.swap(unsafe { own(f) }, Ordering::Relaxed);
    if prev != 0 && crate::nanbox::is_cell(prev) {
        unsafe { torajs_rc::__torajs_rc_dec(crate::nanbox::as_void_ptr(prev)) };
    }
}

/// The cell's call face: forward to the bound closure with the argv
/// padded to [`BOUND_ARITY`]. An unbound id is a catchable TypeError
/// — the injector and the cell mint key off the same member-name
/// predicate, so this is an invariant check, not a fallback.
///
/// # Safety
/// `argv` points at `argc` live any slots.
pub(crate) unsafe fn call_bound(id: i64, argv: *const u64, argc: i64) -> u64 {
    let f = usize::try_from(id)
        .ok()
        .and_then(|i| BOUND.get(i))
        .map_or(0, |s| s.load(Ordering::Relaxed));
    if f == 0 {
        unsafe {
            __torajs_throw_type_error(
                c"builtin namespace static is self-hosted but the program never bound it".as_ptr(),
            );
        }
        return VALUE_UNDEFINED;
    }
    let mut slots = [VALUE_UNDEFINED; BOUND_ARITY];
    for (i, s) in slots.iter_mut().enumerate() {
        *s = unsafe { arg_at(argv, argc, i as i64) };
    }
    unsafe {
        crate::method_call_closure_dispatch::__torajs_any_call(
            f,
            slots.as_ptr(),
            BOUND_ARITY as i64,
        )
    }
}
