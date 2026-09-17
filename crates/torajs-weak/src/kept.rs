//! §9.10 [[KeptAlive]] — AddToKeptObjects / ClearKeptObjects.
//!
//! `new WeakRef(target)` and `ref.deref()` keep their target alive
//! until the current job ends: `new WeakRef({}).deref()` answers the
//! object, however the rest of the program treats it. The list holds
//! one reference per distinct consecutive add
//! and the host releases them all when the synchronous job run is over
//! — the microtask drain calls [`__torajs_kept_objects_clear`] before
//! its first job and after each one.
//!
//! Before this list the typed lane leaked the constructor's temporary
//! argument outright (nothing owned it once the kernel took no
//! reference); an earlier release would have been the other error —
//! the object dying under a `deref()` in the same job.
//!
//! Single mutator today; the atomics keep the §6.2 multi-thread-ready
//! shape, like the FinalizationRegistry cleanup queue next door.

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

static KEPT: AtomicPtr<*mut c_void> = AtomicPtr::new(ptr::null_mut());
static KEPT_LEN: AtomicUsize = AtomicUsize::new(0);
static KEPT_CAP: AtomicUsize = AtomicUsize::new(0);

const SLOT: usize = core::mem::size_of::<*mut c_void>();

unsafe extern "C" {
    #[link_name = "__torajs_realloc"]
    fn realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
    fn __torajs_rc_inc(p: *mut c_void);
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-microtask — the job-boundary hook.
    fn __torajs_kept_clear_install(clear: unsafe extern "C" fn());
}

/// AddToKeptObjects(`target`) — takes a reference of its own.
///
/// # Safety
/// `target` is a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_kept_objects_add(target: *mut c_void) {
    let len = KEPT_LEN.load(Ordering::Relaxed);
    let cap = KEPT_CAP.load(Ordering::Relaxed);
    let mut buf = KEPT.load(Ordering::Relaxed);
    // The list only has to keep each target alive, not count adds: a
    // repeat of the newest entry (`w.deref()` in a loop) is already
    // covered, and without this a synchronous loop grows the list by
    // one slot per call.
    // SAFETY: slot `len - 1` is initialized when `len > 0`.
    if len > 0 && unsafe { *buf.add(len - 1) } == target {
        return;
    }
    if len == cap {
        if cap == 0 {
            // SAFETY: installs a fn of the hook's exact signature.
            unsafe { __torajs_kept_clear_install(__torajs_kept_objects_clear) };
        }
        let new_cap = if cap == 0 { 16 } else { cap * 2 };
        // SAFETY: `buf` is this list's own block of `cap` slots.
        buf = unsafe { realloc(buf.cast(), cap * SLOT, new_cap * SLOT) }.cast();
        KEPT.store(buf, Ordering::Relaxed);
        KEPT_CAP.store(new_cap, Ordering::Relaxed);
    }
    unsafe {
        __torajs_rc_inc(target);
        *buf.add(len) = target;
    }
    KEPT_LEN.store(len + 1, Ordering::Relaxed);
}

/// ClearKeptObjects — release every kept reference. The length resets
/// first: a release never runs user code, but a dying target's
/// observers do run, and none of them may see a half-cleared list.
///
/// # Safety
/// Called only at a job boundary, by the microtask drain.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_kept_objects_clear() {
    let len = KEPT_LEN.swap(0, Ordering::Relaxed);
    let buf = KEPT.load(Ordering::Relaxed);
    for i in 0..len {
        // SAFETY: slots `0..len` each hold one reference this list took.
        unsafe { __torajs_value_drop_heap(*buf.add(i)) };
    }
}
