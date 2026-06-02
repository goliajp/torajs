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
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::layout::{HeapHeader, Promise, STATE_REJECTED, as_promise};

unsafe extern "C" {
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_syscall_write(fd: i32, buf: *const u8, n: usize) -> isize;
    /// `__torajs_str_print_err(s)` — write a Str's payload bytes
    /// plus a trailing newline to stderr. Defined in torajs-str
    /// (`crates/torajs-str/src/print.rs`).
    fn __torajs_str_print_err(s: *const u8);
}

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
fn reason_is_cell_like(v: i64) -> bool {
    const TOP_16_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_BIT_TYPE_OTHER: u64 = 0x02;
    let u = v as u64;
    u != 0 && (u & TOP_16_MASK) == 0 && (u & TAG_BIT_TYPE_OTHER) == 0
}

/// Pending-unhandled list. Every rejected Promise pushes its raw
/// pointer here at reject time; the sweep at `main` exit reads it.
/// Single-threaded runtime today; `Mutex` keeps the API sound for
/// the future multi-threaded story (matches the AtomicPtr / Mutex
/// pattern already used across torajs-promise / torajs-mutex).
static UNHANDLED_LIST: Mutex<Vec<i64>> = Mutex::new(Vec::new());

/// Process-global "has the reporter fired at least once?" flag.
/// Set by [`fire_unhandled_reporter`]; read by
/// [`__torajs_main_exit_code`] to choose `main`'s return value.
static UNHANDLED_REJECTION_OCCURRED: AtomicI32 = AtomicI32::new(0);

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
    UNHANDLED_LIST
        .lock()
        .expect("UNHANDLED_LIST poisoned")
        .push(p as i64);
}

/// Walk the pending-unhandled list once. For every entry whose
/// `has_handler` is still 0 (state confirmed REJECTED defensively
/// for forward-compat), fire the default reporter; drop the rc
/// inc'd at enqueue time regardless. Drains the list to empty.
unsafe fn sweep_unhandled_list() {
    let pending: Vec<i64> = {
        let mut guard = UNHANDLED_LIST.lock().expect("UNHANDLED_LIST poisoned");
        core::mem::take(&mut *guard)
    };
    for ptr in pending {
        let p = ptr as *mut c_void;
        let pp = as_promise(p);
        unsafe {
            if (*pp).state == STATE_REJECTED && (*pp).has_handler == 0 {
                fire_unhandled_reporter(pp);
            }
            crate::pool::__torajs_promise_drop(p);
        }
    }
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
