//! Host PromiseRejectionTracker per spec §27.2.1.9.
//!
//! P10.5-A3-b — every rejected Promise is appended to a process-
//! global "pending unhandled" list at the point of rejection.
//! `attach_then` / `get_value` mark the promise as observed by
//! flipping its `has_handler` byte. At the synthesized `main`'s
//! exit, after the microtask drain + cycle drain, ssa_lower's
//! `main_exit_code` reads through this module, which first
//! sweeps the list (firing the default reporter on every entry
//! whose `has_handler` is still 0) and then returns the
//! process-global flag so `main` returns 1 iff at least one
//! report fired.
//!
//! ## Why a separate list, not the microtask queue
//!
//! `await p` lowers to a synchronous `microtask_run_until_idle`
//! immediately followed by `promise_get_value(p)`
//! (ssa_lower:23814). If the HPRT-check microtask were enqueued
//! at reject time, the `await`'s drain would pop it BEFORE
//! `get_value` had a chance to set `has_handler = 1`, surfacing
//! a spurious unhandled-rejection on every awaited rejection
//! (and every catch-wrapped throw inside an async fn body —
//! P10.5-A1's wrap re-rejects with __async_err). Keeping HPRT
//! off the microtask queue and deferring the sweep until
//! `main`'s exit lets every same-tick observation register first
//! — matching the V8 / SpiderMonkey HPRT timing.
//!
//! ## Wire-up
//!
//! - [`enqueue_hprt_check`] — called from
//!   `state::__torajs_promise_reject` (PENDING → REJECTED) and
//!   `pool::__torajs_promise_alloc_rejected{,_heap}` (rejected
//!   at construction). Bumps refcount + pushes the promise
//!   pointer onto [`UNHANDLED_LIST`].
//! - [`__torajs_main_exit_code`] — read by ssa_lower's
//!   synthesized `main` just before `ret`. Sweeps
//!   [`UNHANDLED_LIST`], fires [`fire_unhandled_reporter`] on
//!   every entry still un-observed, drops the rc bumped at
//!   enqueue time, then returns `UNHANDLED_REJECTION_OCCURRED`.
//! - [`compact_observed`] — deferring the sweep to exit must not
//!   mean deferring the RELEASE to exit: an observed entry is
//!   already decided, so the growth step drops it early instead of
//!   pinning it, its reason and the reason's whole graph until the
//!   process ends.

use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::layout::{STATE_REJECTED, as_promise};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
    /// `__torajs_str_print_err(s)` — write a Str's payload bytes
    /// plus a trailing newline to stderr. Defined in torajs-str
    /// (`crates/torajs-str/src/print.rs`).
    fn __torajs_str_print_err(s: *const u8);
    /// `__torajs_str_write_err(s)` — the newline-free twin of
    /// `__torajs_str_print_err`, for composing an Error's `name` and
    /// `message` into a single reported line. NULL → no-op. Same
    /// UTF-8 transcoding, so a Latin-1 / UTF-16 `message` renders
    /// correctly rather than as raw payload bytes.
    fn __torajs_str_write_err(s: *const u8);
    /// torajs-anyvalue — an error instance's `name` resolved through
    /// its own slot and then the class prototype chain (§20.5.3.2);
    /// the `torajs_throw::uncaught` twin declares the same symbol.
    /// Borrowed Str; never NULL.
    fn __torajs_error_name_get(obj: *const u8) -> *const u8;
    /// torajs-str — own-absence sentinel identity probe; see the
    /// `torajs_throw::uncaught` twin for why the raw length read of
    /// the `message` slot is not enough.
    fn __torajs_str_is_undef(p: *const u8) -> i64;
    /// torajs-mmalloc libc-compat realloc / sized free — same
    /// externs `torajs_microtask::mt_grow` and `pool.rs` use.
    #[link_name = "__torajs_realloc"]
    fn tr_realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
    #[link_name = "__torajs_free"]
    fn tr_free(p: *mut c_void, size: usize);
}

/// Simple-fn `(reason: AnyValue) -> void` cb shape registered
/// through `__torajs_process_on_unhandled_rejection_register_simple`.
type SimpleCb = unsafe extern "C" fn(reason: i64);

/// Closure cb shape — env+8 carries the user fn pointer
/// `extern "C" fn(env: *mut c_void, reason: i64)` (same env layout
/// as `promise_then_closure` / `microtask_enqueue_closure`).
// S1 (RFC 20260810-indirect-argc-abi) — env-first entry: hidden
// argc rides between the env and the reason.
type ClosureCb = unsafe extern "C" fn(env: *mut c_void, argc: i64, reason: i64);

/// Universal heap-header `type_tag` for Str (0). Mirrors
/// `torajs_rc::Tag::Str`. Re-declared as a `u16` constant to keep
/// this crate free of a torajs-rc dep (same independent-knowledge
/// pattern `combinator.rs` uses for Array layout).
pub(crate) const TAG_STR: u16 = 0;

/// Universal heap-header `type_tag` for class instances
/// (`torajs_rc::Tag::Obj`). Same mirrored-constant convention as
/// [`TAG_STR`]; the value also appears as
/// `layout::ALLSETTLED_OBJ_TAG`.
pub(crate) const TAG_OBJ: u16 = 1;

/// `torajs_rc::FLAG_ERROR` — header bit 7, set on Error-derived
/// class instances by ssa_lower's `__new_<C>` factory codegen. It
/// IS tr's [[ErrorData]] internal slot. Mirror of
/// `torajs_throw::uncaught::FLAG_ERROR` (the uncaught-throw twin of
/// this reporter) and `torajs_meta::struct_reflect::FLAG_ERROR`.
pub(crate) const FLAG_ERROR: u16 = 1 << 7;

/// Error instance field offsets. Obj layout is
/// `[header:32][field0:8][field1:8]…`, and Error declares `message`
/// first, then `name` — both Str pointers. Lockstep mirror of
/// `torajs_throw::uncaught::OBJ_MESSAGE_OFF`; the header size is the
/// same 32 as `layout::ALLSETTLED_OBJ_HEADER_SIZE`. `name` is
/// resolved rather than read at an offset — see the twin.
pub(crate) const OBJ_MESSAGE_OFF: usize = 32;

/// Str `length` field offset. The block is
/// `[header:8][length:4][_pad:4][bytes:N]` — the count is a **u32**,
/// and the u32 beside it is reserved padding whose value readers
/// must not assume (`torajs_str::layout::STR_PAD_OFF`), so this is
/// read at u32 width, never as a u64 spanning both.
///
/// Read only to decide whether a `message` is empty; the payload
/// itself goes through `__torajs_str_write_err`, which does its own
/// encoding-aware slicing.
pub(crate) const STR_LEN_OFF: usize = 8;

/// NaN-box "cell-like" gate — mirrors
/// `torajs_value_drop::nan_box_is_cell_like` /
/// `torajs_anyvalue::nanbox::is_cell` / `torajs_rc::nan_box_is_cell_like`.
///
/// A `Promise.reject(<Type::Any value>)` flows through
/// ssa_lower:17590's `Type::Any` is_heap=true arm into
/// `__torajs_promise_alloc_rejected_heap`, but the i64 slot in that
/// path receives a NaN-box `u64` *immediate* (not a real heap
/// pointer): top 16 bits carry the NaN tag, so dereferencing the
/// implied `HeapHeader.type_tag` would walk into kernel-mapped
/// address space and SIGSEGV. `__torajs_value_drop_heap` already
/// skips on the same gate; the reporter must too.
#[inline]
pub(crate) fn reason_is_cell_like(v: i64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let u = v as u64;
    u != 0 && (u & TOP_16_MASK) == 0 && (u & TAG_BIT_TYPE_OTHER) == 0
}

/// Pending-unhandled list backing array. NULL until first push;
/// grown by doubling through mmalloc's `__torajs_realloc` — the
/// same AtomicPtr + AtomicUsize shape as `torajs_microtask::
/// MT_QUEUE` (single-threaded runtime today, Relaxed ordering;
/// pre-pays the multi-threaded story per the §6.2 policy).
///
/// SIGBUS fix (2026-07-03) — this was `static Mutex<Vec<i64>>`.
/// std's pthread Mutex lazy-allocates its inner pthread_mutex_t
/// via libc malloc on first `lock()`, which made the `main`-exit
/// sweep the ONLY libc-malloc call in the whole AOT binary
/// (everything else is mmalloc). Under parallel conformance-gate
/// VM layouts, libsystem_malloc's mfm allocator intermittently
/// faulted on its own guard page inside that one alloc
/// (KERN_PROTECTION_FAILURE / exit 138 — the regex-017-sticky
/// flaky family; 6 crash reports share the identical stack:
/// mfm_alloc ← OnceBox<Mutex>::initialize ←
/// __torajs_main_exit_code). The atomic + mmalloc shape removes
/// the libc-malloc dependency from the exit path entirely.
static UNHANDLED_PTR: AtomicPtr<i64> = AtomicPtr::new(core::ptr::null_mut());
static UNHANDLED_LEN: AtomicUsize = AtomicUsize::new(0);
static UNHANDLED_CAP: AtomicUsize = AtomicUsize::new(0);

/// Process-global "has the reporter fired at least once?" flag.
/// Set by [`fire_unhandled_reporter`]; read by
/// [`__torajs_main_exit_code`] to choose `main`'s return value.
/// A user-side `process.on('unhandledRejection', cb)` handler
/// suppresses the default reporter AND the flag — bun's behaviour
/// when a listener is registered.
pub(crate) static UNHANDLED_REJECTION_OCCURRED: AtomicI32 = AtomicI32::new(0);

/// `process.on('unhandledRejection', cb)` listener slot. `KIND`
/// 0 → no listener (default reporter runs);
/// 1 → simple fn (`PTR` holds the raw `fn(reason: i64)` pointer);
/// 2 → closure (`PTR` holds the env block; env+8 carries the user
///     `fn(env, reason)` ptr, refcount inc'd at register time so
///     captures live across the sweep).
///
/// Single-threaded runtime → Relaxed ordering is sufficient; the
/// AtomicPtr/AtomicU8 pair pre-pays the future multi-threaded
/// story per the project-wide static-mut → atomic policy
/// (`torajs_microtask::MT_QUEUE`, `torajs_arr::pool`, ...).
const CB_KIND_NONE: u8 = 0;
const CB_KIND_SIMPLE: u8 = 1;
const CB_KIND_CLOSURE: u8 = 2;
static UNHANDLED_CB_PTR: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
static UNHANDLED_CB_KIND: AtomicU8 = AtomicU8::new(CB_KIND_NONE);

/// Replace the active listener. Releases the prior closure env's
/// refcount (if any) before overwriting; simple-fn slot has no rc
/// bookkeeping (function pointers live in the binary's .text).
unsafe fn install_listener(kind: u8, ptr: *mut c_void) {
    let prior_kind = UNHANDLED_CB_KIND.swap(kind, Ordering::Relaxed);
    let prior_ptr = UNHANDLED_CB_PTR.swap(ptr, Ordering::Relaxed);
    if prior_kind == CB_KIND_CLOSURE && !prior_ptr.is_null() {
        unsafe { __torajs_value_drop_heap(prior_ptr) };
    }
}

/// `process.on('unhandledRejection', simpleFn)` — register a named
/// fn (no captures). `fn_ptr` lives in `.text`, no refcount.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_on_unhandled_rejection_register_simple(
    fn_ptr: *mut c_void,
) {
    unsafe { install_listener(CB_KIND_SIMPLE, fn_ptr) };
}

/// `process.on('unhandledRejection', closure)` — register a
/// capturing lambda. Inc the env's rc so captures survive across
/// the deferred sweep at `main` exit; `install_listener` releases
/// the prior closure's env (if any).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_process_on_unhandled_rejection_register_closure(
    env: *mut c_void,
) {
    if !env.is_null() {
        unsafe { __torajs_rc_inc(env) };
    }
    unsafe { install_listener(CB_KIND_CLOSURE, env) };
}

/// NaN-box int32 tag — top 16 bits identify a tagged integer payload
/// in the low 32 bits. Mirrors `torajs_anyvalue::nanbox::
/// TAG_TYPE_NUMBER` + `box_int32`; re-declared as a `u64` constant
/// to keep this crate free of a torajs-anyvalue dep (same
/// independent-knowledge pattern as `TAG_STR` / `reason_is_cell_like`
/// above).
const NAN_TAG_TYPE_NUMBER: u64 = 0xFFFE_0000_0000_0000;

/// Mirrors `torajs_anyvalue::nanbox::VALUE_NULL` (`TAG_BIT_TYPE_OTHER`).
const NAN_VALUE_NULL: i64 = 0x02;

/// Lift the rejected promise's `value` slot into the AnyValue
/// NaN-box wire form expected by user-registered handlers. The
/// reasoning mirrors `ssa_lower:17590` which routes `Type::Any`
/// reasons through the `_heap` alloc variant as opaque NaN-box
/// immediates: a `value_is_heap=1` reason is already a valid
/// AnyValue (cell-tagged real heap pointer OR a NaN-box
/// immediate); `value_is_heap=0` reasons are raw primitive bits
/// — narrow MVP boxes them as Int32 when non-zero and as
/// `VALUE_NULL` when zero (the default `Promise.reject()` /
/// `Promise.all([pending])` sentinel).
fn box_reason_as_anyvalue(reason: i64, is_heap: u8) -> i64 {
    if is_heap != 0 {
        return if reason == 0 { NAN_VALUE_NULL } else { reason };
    }
    if reason == 0 {
        return NAN_VALUE_NULL;
    }
    // Truncating cast to i32 — narrow MVP; reasons outside i32 range
    // surface as their low-32 bits (caller-visible documented limit).
    (NAN_TAG_TYPE_NUMBER | ((reason as u32) as u64)) as i64
}

/// Append `p` to the pending-unhandled list. Caller must have
/// already transitioned `p` to STATE_REJECTED. Refcount on `p` is
/// inc'd here so the sweep sees a live block even if every caller
/// has dropped their ref; [`sweep_unhandled_list`] pairs that with
/// a `promise_drop` after the check.
#[inline]
pub(crate) unsafe fn enqueue_hprt_check(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe { __torajs_rc_inc(p) };
    let mut len = UNHANDLED_LEN.load(Ordering::Relaxed);
    let cap = UNHANDLED_CAP.load(Ordering::Relaxed);
    if len == cap {
        if cap != 0 {
            len = unsafe { compact_observed(len) };
        }
        // Double when the scan did not buy back at least half the
        // buffer — that is what keeps the push amortised O(1) once
        // compaction stops paying (a program that never observes its
        // rejections reclaims nothing and grows exactly as before).
        if cap == 0 || len > cap / 2 {
            let new_cap = if cap == 0 { 8 } else { cap * 2 };
            let elem = core::mem::size_of::<i64>();
            let cur = UNHANDLED_PTR.load(Ordering::Relaxed);
            let new_buf =
                unsafe { tr_realloc(cur as *mut c_void, cap * elem, new_cap * elem) } as *mut i64;
            UNHANDLED_PTR.store(new_buf, Ordering::Relaxed);
            UNHANDLED_CAP.store(new_cap, Ordering::Relaxed);
        }
    }
    let buf = UNHANDLED_PTR.load(Ordering::Relaxed);
    unsafe {
        *buf.add(len) = p as i64;
    }
    UNHANDLED_LEN.store(len + 1, Ordering::Relaxed);
}

/// Drop every entry the exit sweep would already walk past, keeping
/// the survivors packed at the front. Returns the new length.
///
/// `has_handler` only ever goes 0 → 1 while an entry is on the list:
/// the rc this list holds keeps the cell out of the pool's recycler,
/// and clearing the byte back to 0 happens only in a fresh
/// allocation. So an observed entry can never become unobserved, the
/// sweep is guaranteed to skip it, and the only thing it would still
/// do to it there is the very `promise_drop` done here. Doing it at
/// the growth step instead frees the promise, its reason and
/// everything the reason holds when the handler attaches rather than
/// at process exit — the difference between a loop that rejects and
/// catches N promises costing O(1) and costing O(N). `await`ing a
/// rejection inside a 200k-iteration loop held 52 MB.
///
/// Nothing here can re-enter the list: `promise_drop` frees cells, and
/// freeing a cell never rejects a promise. The user listener, which
/// can, runs in the sweep and not here.
unsafe fn compact_observed(len: usize) -> usize {
    let buf = UNHANDLED_PTR.load(Ordering::Relaxed);
    if buf.is_null() {
        return len;
    }
    let mut keep = 0usize;
    for i in 0..len {
        let p = unsafe { *buf.add(i) } as *mut c_void;
        let pp = as_promise(p);
        if unsafe { (*pp).has_handler } != 0 {
            unsafe { crate::pool::__torajs_promise_drop(p) };
            continue;
        }
        unsafe { *buf.add(keep) = p as i64 };
        keep += 1;
    }
    UNHANDLED_LEN.store(keep, Ordering::Relaxed);
    keep
}

/// Walk the pending-unhandled list once. For every entry whose
/// `has_handler` is still 0 (state confirmed REJECTED defensively
/// for forward-compat), dispatch to a user-registered
/// `'unhandledRejection'` listener if one was installed; otherwise
/// fire the default reporter. Always drops the rc inc'd at
/// enqueue time. Drains the list to empty.
unsafe fn sweep_unhandled_list() {
    // Detach the whole backing array first (same semantics as the
    // previous `mem::take` of the Vec): re-entrant enqueues from a
    // user listener land in a fresh buffer, not the one being swept.
    let buf = UNHANDLED_PTR.swap(core::ptr::null_mut(), Ordering::Relaxed);
    let len = UNHANDLED_LEN.swap(0, Ordering::Relaxed);
    let cap = UNHANDLED_CAP.swap(0, Ordering::Relaxed);
    if buf.is_null() {
        return;
    }
    for i in 0..len {
        let p = unsafe { *buf.add(i) } as *mut c_void;
        let pp = as_promise(p);
        unsafe {
            if (*pp).state == STATE_REJECTED && (*pp).has_handler == 0 {
                let kind = UNHANDLED_CB_KIND.load(Ordering::Relaxed);
                if kind == CB_KIND_NONE {
                    crate::unhandled_report::fire_unhandled_reporter(pp);
                } else {
                    let reason = box_reason_as_anyvalue((*pp).value, (*pp).value_is_heap);
                    let cb_ptr = UNHANDLED_CB_PTR.load(Ordering::Relaxed);
                    if kind == CB_KIND_SIMPLE && !cb_ptr.is_null() {
                        let cb: SimpleCb = core::mem::transmute(cb_ptr);
                        cb(reason);
                    } else if kind == CB_KIND_CLOSURE && !cb_ptr.is_null() {
                        // env+8 — same env layout as
                        // `promise_then_closure` / `microtask_enqueue_closure`.
                        let fn_addr = *((cb_ptr as *mut u8).add(8) as *const *mut c_void);
                        if !fn_addr.is_null() {
                            let cb: ClosureCb = core::mem::transmute(fn_addr);
                            cb(cb_ptr, 1, reason);
                        }
                    }
                    // A user listener suppresses the default exit-code
                    // signal — bun returns 0 when a handler is registered
                    // even if the listener doesn't itself re-throw.
                }
            }
            crate::pool::__torajs_promise_drop(p);
        }
    }
    unsafe { tr_free(buf as *mut c_void, cap * core::mem::size_of::<i64>()) };
}

/// `main`'s exit code — first sweeps any pending unhandled
/// rejections (firing the default reporter on each), then returns
/// `0` if no reporter ever fired during this process's lifetime
/// or `1` otherwise. ssa_lower emits a call to this just before
/// `ret` in the synthesized `main` body, after the microtask drain
/// + cycle drain, so the process exit status matches Bun's
/// `error: <reason>` + exit-1 behaviour.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_main_exit_code() -> i32 {
    unsafe { sweep_unhandled_list() };
    UNHANDLED_REJECTION_OCCURRED.load(Ordering::Relaxed)
}
