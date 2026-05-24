//! Microtask queue substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate of the architecture rewrite (P5, 2026-05-24).
//! Port of the `T-15.c microtask queue` section of
//! `runtime_promise.c` — that section moves here; the rest of the
//! Promise surface stays C-side until P6.1.
//!
//! ## Algorithm
//!
//! Single global FIFO queue. Tasks pushed at the tail via
//! `__torajs_microtask_enqueue`, popped at the head via the drain
//! loop in `__torajs_microtask_run_until_idle`. Backing array
//! grows by doubling (starts at 32 slots); compaction by `memmove`
//! when the head cursor passes half-capacity.
//!
//! The `fn` signature is `void (*)(int64_t arg)` — a single i64
//! slot carries either a primitive value or a heap pointer cast
//! through `(int64_t)(intptr_t)`. Codegen for `await` (T-16) and
//! `.then` (T-15.d) both pack `{Promise *, callback closure}` into
//! the arg slot via a small heap struct.
//!
//! Drain semantics: `run_until_idle` returns ONLY when the queue
//! is empty, including tasks enqueued during the drain — matches
//! JS spec's microtask draining (drain to empty before yielding to
//! the event loop / before exit).
//!
//! ## `static mut` 替代 → `AtomicPtr`
//!
//! Same rationale as `torajs-weak::registry` / `torajs-cycle::buffer` /
//! `torajs-arr::pool` — single-threaded runtime today but Rust 2024
//! deprecates raw `static mut`. `AtomicPtr` + `AtomicUsize` + `Relaxed`
//! compiles to identical loads/stores while keeping `&'static` APIs
//! sound + pre-paying the future multi-threaded story.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

/// Public typedef of the microtask function pointer. Matches
/// `runtime_promise.c::__torajs_microtask_fn_t = void (*)(int64_t)`.
/// Kept as a plain `extern "C" fn` so call sites (C-side
/// runtime_promise.c, eventually Rust-side P6.1) get the same ABI.
pub type MicrotaskFn = unsafe extern "C" fn(arg: i64);

/// One queue entry: a function pointer + the i64 arg slot.
#[repr(C)]
#[derive(Copy, Clone)]
struct Microtask {
    fn_: MicrotaskFn,
    arg: i64,
}

/// Backing array pointer. NULL until first push; `realloc`'d to
/// `MT_CAP × sizeof(Microtask)` on growth.
static MT_QUEUE: AtomicPtr<Microtask> = AtomicPtr::new(ptr::null_mut());

/// Head cursor — index of the next task to pop. Bumps on each
/// drain step; compaction resets it to 0.
static MT_HEAD: AtomicUsize = AtomicUsize::new(0);

/// Live tail — `mt_queue[mt_head..mt_len]` is the pending slice.
static MT_LEN: AtomicUsize = AtomicUsize::new(0);

/// Allocated capacity, in entries. Doubles from 32 on growth.
static MT_CAP: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

/// Grow the backing array (double capacity, starting at 32).
/// Single-threaded — no concurrent grow possible.
fn mt_grow() {
    let cap = MT_CAP.load(Ordering::Relaxed);
    let new_cap = if cap == 0 { 32 } else { cap * 2 };
    let cur = MT_QUEUE.load(Ordering::Relaxed);
    let new_buf = unsafe {
        realloc(
            cur as *mut c_void,
            new_cap * core::mem::size_of::<Microtask>(),
        )
    } as *mut Microtask;
    MT_QUEUE.store(new_buf, Ordering::Relaxed);
    MT_CAP.store(new_cap, Ordering::Relaxed);
}

/// Compact the queue when the head cursor wanders past half-cap.
/// Two cases:
///   - `head >= len`: queue is logically empty; reset both to 0
///     so the next enqueue starts at index 0 of the existing buffer
///     (avoids the pre-fix SIGBUS at chain length 33+ when the
///     unconditional `mt_queue[len++]` wrote past `cap`).
///   - `head > 0` with `live > 0`: `memmove` live slice down to
///     index 0, reset head.
fn mt_compact() {
    let head = MT_HEAD.load(Ordering::Relaxed);
    let len = MT_LEN.load(Ordering::Relaxed);
    if head >= len {
        MT_HEAD.store(0, Ordering::Relaxed);
        MT_LEN.store(0, Ordering::Relaxed);
        return;
    }
    if head == 0 {
        return;
    }
    let live = len - head;
    let buf = MT_QUEUE.load(Ordering::Relaxed);
    unsafe {
        core::ptr::copy(buf.add(head), buf, live);
    }
    MT_LEN.store(live, Ordering::Relaxed);
    MT_HEAD.store(0, Ordering::Relaxed);
}

/// Enqueue `(fn, arg)` at the tail of the microtask queue. NULL
/// `fn` is a defensive silent no-op (matches C contract). Triggers
/// `mt_compact` first if the head has wandered past half-cap (cheap
/// recovery); otherwise grows by doubling.
///
/// # Safety
/// `fn_` must be a valid C function pointer with the
/// `extern "C" fn(arg: i64)` signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_enqueue(fn_: Option<MicrotaskFn>, arg: i64) {
    let Some(fn_) = fn_ else { return };
    let len = MT_LEN.load(Ordering::Relaxed);
    let cap = MT_CAP.load(Ordering::Relaxed);
    if len == cap {
        let head = MT_HEAD.load(Ordering::Relaxed);
        if head > cap / 2 {
            mt_compact();
        } else {
            mt_grow();
        }
    }
    let buf = MT_QUEUE.load(Ordering::Relaxed);
    let len = MT_LEN.load(Ordering::Relaxed);
    unsafe {
        *buf.add(len) = Microtask { fn_, arg };
    }
    MT_LEN.store(len + 1, Ordering::Relaxed);
}

/// Drain the microtask queue to empty. Pops one task, runs it,
/// repeats — new tasks enqueued during the callback land at the
/// tail and get processed in this same drain (JS spec microtask
/// semantics: drain to empty before yielding to event loop / exit).
///
/// Auto-called from synthesized `main` at program exit by
/// codegen (T-15.e). After the drain, head + len are both reset
/// to 0 so the next enqueue starts at index 0.
///
/// # Safety
/// Calls user-provided fn pointers; each fn must be a valid
/// `extern "C" fn(i64)` that doesn't reenter `microtask_enqueue`
/// in a way that violates the single-threaded invariant.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_run_until_idle() {
    loop {
        let head = MT_HEAD.load(Ordering::Relaxed);
        let len = MT_LEN.load(Ordering::Relaxed);
        if head >= len {
            break;
        }
        let buf = MT_QUEUE.load(Ordering::Relaxed);
        let t = unsafe { *buf.add(head) };
        MT_HEAD.store(head + 1, Ordering::Relaxed);
        unsafe { (t.fn_)(t.arg) };
        let new_head = MT_HEAD.load(Ordering::Relaxed);
        let cap = MT_CAP.load(Ordering::Relaxed);
        if new_head > 64 && new_head > cap / 2 {
            mt_compact();
        }
    }
    MT_HEAD.store(0, Ordering::Relaxed);
    MT_LEN.store(0, Ordering::Relaxed);
}

/// `mt_len - mt_head` — number of pending microtasks. Returns
/// `usize` to match the C `size_t` signature.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_microtask_pending_count() -> usize {
    let head = MT_HEAD.load(Ordering::Relaxed);
    let len = MT_LEN.load(Ordering::Relaxed);
    len - head
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Tests run sequentially because they share the process-global
    // queue. `cargo test --release` (LTO=fat) builds one shared
    // executable per crate, and these tests mutate the static
    // queue state. Use a mutex to serialize.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_queue() {
        MT_HEAD.store(0, Ordering::Relaxed);
        MT_LEN.store(0, Ordering::Relaxed);
    }

    static mut COUNTER: i64 = 0;

    unsafe extern "C" fn add(arg: i64) {
        unsafe {
            COUNTER += arg;
        }
    }

    #[test]
    fn enqueue_then_drain_sums_args() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe { COUNTER = 0 };
        for v in [1, 2, 3, 4, 5] {
            unsafe { __torajs_microtask_enqueue(Some(add), v) };
        }
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 5);
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
        assert_eq!(unsafe { COUNTER }, 1 + 2 + 3 + 4 + 5);
    }

    static mut REENQUEUE_BUDGET: i32 = 0;

    unsafe extern "C" fn reenqueue(arg: i64) {
        unsafe {
            COUNTER += arg;
            if REENQUEUE_BUDGET > 0 {
                REENQUEUE_BUDGET -= 1;
                __torajs_microtask_enqueue(Some(reenqueue), arg + 1);
            }
        }
    }

    #[test]
    fn drain_processes_reentrant_enqueues() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe {
            COUNTER = 0;
            REENQUEUE_BUDGET = 3;
            __torajs_microtask_enqueue(Some(reenqueue), 10);
            __torajs_microtask_run_until_idle();
        }
        // initial 10, then 11, 12, 13 (budget=3)
        assert_eq!(unsafe { COUNTER }, 10 + 11 + 12 + 13);
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }

    unsafe extern "C" fn noop(_arg: i64) {}

    #[test]
    fn enqueue_above_initial_cap_triggers_grow() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        for i in 0..200 {
            unsafe { __torajs_microtask_enqueue(Some(noop), i) };
        }
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 200);
        unsafe { __torajs_microtask_run_until_idle() };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }

    #[test]
    fn null_fn_is_silent_noop() {
        let _g = TEST_LOCK.lock().unwrap();
        reset_queue();
        unsafe { __torajs_microtask_enqueue(None, 42) };
        assert_eq!(unsafe { __torajs_microtask_pending_count() }, 0);
    }
}
