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

use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU8, AtomicUsize, Ordering};

use crate::layout::{HeapHeader, Promise, STATE_REJECTED, as_promise};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
    /// `__torajs_str_print_err(s)` — write a Str's payload bytes
    /// plus a trailing newline to stderr. Defined in torajs-str
    /// (`crates/torajs-str/src/print.rs`).
    fn __torajs_str_print_err(s: *const u8);
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
type ClosureCb = unsafe extern "C" fn(env: *mut c_void, reason: i64);

/// Universal heap-header `type_tag` for Str (0). Mirrors
/// `torajs_rc::Tag::Str`. Re-declared as a `u16` constant to keep
/// this crate free of a torajs-rc dep (same independent-knowledge
/// pattern `combinator.rs` uses for Array layout).
const TAG_STR: u16 = 0;

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
static UNHANDLED_REJECTION_OCCURRED: AtomicI32 = AtomicI32::new(0);

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
    let len = UNHANDLED_LEN.load(Ordering::Relaxed);
    let cap = UNHANDLED_CAP.load(Ordering::Relaxed);
    if len == cap {
        let new_cap = if cap == 0 { 8 } else { cap * 2 };
        let elem = core::mem::size_of::<i64>();
        let cur = UNHANDLED_PTR.load(Ordering::Relaxed);
        let new_buf =
            unsafe { tr_realloc(cur as *mut c_void, cap * elem, new_cap * elem) } as *mut i64;
        UNHANDLED_PTR.store(new_buf, Ordering::Relaxed);
        UNHANDLED_CAP.store(new_cap, Ordering::Relaxed);
    }
    let buf = UNHANDLED_PTR.load(Ordering::Relaxed);
    unsafe {
        *buf.add(len) = p as i64;
    }
    UNHANDLED_LEN.store(len + 1, Ordering::Relaxed);
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
                    fire_unhandled_reporter(pp);
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
                            cb(cb_ptr, reason);
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

/// Default unhandled-rejection reporter. Writes `error: <reason>\n`
/// to stderr and sets the process-global flag. Reason rendering:
///   - heap Str (real heap pointer, type_tag == 0) → reuse
///     `__torajs_str_print_err`, prefixed.
///   - other real heap (Obj / Closure / RegExp / Date / Symbol /
///     etc.) → `error: <object>\n` placeholder.
///   - NaN-box immediate (a Type::Any reason routed through the
///     `_heap` alloc path) → `error: <any>\n` placeholder.
///   - primitive (`value_is_heap == 0`) non-zero → `error: <value>\n`
///     with the i64 written as decimal via the local int formatter.
///   - primitive zero → `error: null\n` (bun-parity for the
///     0-arg `Promise.reject()` sentinel).
///
/// v0.5 narrow MVP renderer; A4 lands user-side `process.on(
/// 'unhandledRejection', cb)` which gets the raw reason in
/// NaN-box form and lets user code stringify with full
/// anyvalue dispatch.
unsafe fn fire_unhandled_reporter(pp: *mut Promise) {
    const PREFIX: &[u8] = b"error: ";
    const OBJ_PLACEHOLDER: &[u8] = b"error: <object>\n";
    const ANY_PLACEHOLDER: &[u8] = b"error: <any>\n";
    const NULL_PLACEHOLDER: &[u8] = b"error: null\n";

    UNHANDLED_REJECTION_OCCURRED.store(1, Ordering::Relaxed);

    let reason = unsafe { (*pp).value };
    let is_heap = unsafe { (*pp).value_is_heap };

    if is_heap != 0 && reason != 0 {
        // Type::Any reasons walk through `alloc_rejected_heap` but
        // carry a NaN-box immediate, not a real heap pointer. The
        // cell-like gate isolates real pointers; anything else is
        // surfaced via the `<any>` placeholder so dereferencing the
        // (non-)header can't SIGSEGV.
        if !reason_is_cell_like(reason) {
            unsafe {
                __torajs_syscall_write(2, ANY_PLACEHOLDER.as_ptr(), ANY_PLACEHOLDER.len());
            }
            return;
        }
        let header = reason as *const HeapHeader;
        let type_tag = unsafe { (*header).type_tag };
        if type_tag == TAG_STR {
            unsafe { __torajs_syscall_write(2, PREFIX.as_ptr(), PREFIX.len()) };
            // `str_print_err` already appends a newline.
            unsafe { __torajs_str_print_err(reason as *const u8) };
            return;
        }
        unsafe {
            __torajs_syscall_write(2, OBJ_PLACEHOLDER.as_ptr(), OBJ_PLACEHOLDER.len());
        }
        return;
    }

    if reason == 0 && is_heap == 0 {
        // i64 0 + non-heap — spec-wise this is "rejected with the
        // integer 0", but in practice this is the default-reject
        // sentinel for null / unknown reasons (Promise.reject(),
        // Promise.all([pending]), thenable absorption of pending).
        // Bun prints `null` for these, so we match.
        unsafe { __torajs_syscall_write(2, NULL_PLACEHOLDER.as_ptr(), NULL_PLACEHOLDER.len()) };
        return;
    }

    // Primitive non-zero — format reason as a signed decimal.
    unsafe {
        __torajs_syscall_write(2, PREFIX.as_ptr(), PREFIX.len());
        write_i64_decimal_stderr(reason);
        __torajs_syscall_write(2, b"\n".as_ptr(), 1);
    }
}

/// Write `n` as a signed decimal to stderr. Stack buffer is sized
/// for `i64::MIN` (`-9223372036854775808`, 20 chars) + sign. Single
/// `write(2)` keeps the line atomic.
unsafe fn write_i64_decimal_stderr(n: i64) {
    let mut buf = [0u8; 21];
    let mut idx = buf.len();
    let (mut abs, negative) = if n < 0 {
        // Use wrapping_neg so i64::MIN doesn't trap; its abs as u64
        // is exactly 1 << 63 which fits unsigned.
        (n.wrapping_neg() as u64, true)
    } else {
        (n as u64, false)
    };
    if abs == 0 {
        idx -= 1;
        buf[idx] = b'0';
    } else {
        while abs > 0 {
            idx -= 1;
            buf[idx] = b'0' + (abs % 10) as u8;
            abs /= 10;
        }
    }
    if negative {
        idx -= 1;
        buf[idx] = b'-';
    }
    let n_bytes = buf.len() - idx;
    unsafe {
        __torajs_syscall_write(2, buf.as_ptr().add(idx), n_bytes);
    }
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
