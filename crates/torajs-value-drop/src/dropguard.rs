//! Bounded native stack depth for the release of deep object graphs.
//!
//! A release that takes a container's count to zero releases each of
//! its children, so a chain of a million nested containers (a linked
//! list, an array of arrays, a map whose value is a map, a closure that
//! captures a closure) nests a million native frames and overruns the
//! stack. A release that can nest tests [`below_floor`] on entry: one
//! compare of the stack pointer against a floor near the bottom of the
//! thread's stack. Below it the release goes to [`release_near_floor`],
//! which pushes it onto a pending stack instead of running it, and the
//! outermost deferral runs the pending releases one after another in a
//! loop at that depth, so the native depth stays bounded whatever the
//! depth of the graph. This is CPython's trashcan (`Py_TRASHCAN_BEGIN`)
//! keyed on the stack pointer, as CPython 3.14 keys it, rather than on
//! a depth counter that every release would have to keep. Generated
//! code reaches it through [`__torajs_drop_release`].
//!
//! A pending entry is a cell and a function that releases one
//! reference of it, and the entry owns that reference. A release runs
//! no user code, so running it later, within the same outermost
//! release, cannot be observed. The pending reference also keeps the
//! cell out of a cycle collection that a nested release may trigger:
//! the collector sees one reference it cannot attribute to another
//! cell.
//!
//! The JS runtime is single-threaded, and an AOT staticlib runs no std
//! runtime init (no `thread_local!`), so the state below is
//! process-wide atomics with relaxed ordering.
//!
//! It lives in the dispatcher's own crate: every relocation and every
//! member of the release path is walked by each `tr build` link, so
//! the state is one static the dispatcher reaches without a GOT entry,
//! the slow leg reaches it from one base register, and nothing here
//! is a member of its own.

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering::Relaxed};

/// Releases one reference of a cell.
pub type ReleaseFn = unsafe extern "C" fn(*mut c_void);

/// The floor before the first test: every stack pointer is below it,
/// so the first test takes the slow leg, which sets a provisional
/// floor. It leaves room for the slack the slow leg adds to a floor.
const UNSET: usize = usize::MAX - DEFER_SLACK;

/// How far below the first test's stack pointer the provisional floor
/// sits. The real floor costs a system call, which a program whose
/// releases never nest this deep never makes. Less than the smallest
/// margin of a stack of 256 KiB or more, so a provisional floor below
/// the real one still stops the walk short of the stack's end.
const PROVISIONAL_SLACK: usize = 64 << 10;

/// Stack kept free below the floor for the deferral loop and the
/// bounded nesting under it, and for the argument block the kernel
/// puts above the outermost frame.
const MARGIN_MAX: usize = 2 << 20;

/// How far a release run from the loop may nest before its own
/// releases are deferred in turn: the loop lowers the floor to this
/// much below itself while it runs, so the release it runs passes the
/// test it starts with.
const LOOP_SLACK: usize = 4 << 10;

/// How far above the floor the slow leg already defers. A caller that
/// found itself below the floor reaches the slow leg a frame or two
/// higher (a tail call pops its frame), and a "no" there must not send
/// it back to a test it fails again.
const DEFER_SLACK: usize = 1 << 10;

/// All the state, in one static: one symbol and one base address for
/// the code that reads it.
struct State {
    floor: AtomicUsize,
    pending: AtomicPtr<Pending>,
    len: AtomicUsize,
    cap: AtomicUsize,
    draining: AtomicBool,
}

static S: State = State {
    floor: AtomicUsize::new(UNSET),
    pending: AtomicPtr::new(core::ptr::null_mut()),
    len: AtomicUsize::new(0),
    cap: AtomicUsize::new(0),
    draining: AtomicBool::new(false),
};

unsafe extern "C" {
    fn __torajs_realloc(p: *mut c_void, old_size: usize, new_size: usize) -> *mut c_void;
}

/// Is the stack pointer below the floor? A release that can nest asks
/// on entry and, when it is, hands itself to [`release_near_floor`].
#[inline(always)]
pub(crate) fn below_floor() -> bool {
    stack_ptr() < S.floor.load(Relaxed)
}

/// Release one reference of `p` through `f`, deferred when the stack
/// pointer is below the floor.
///
/// # Safety
/// `f` releases one reference of `p`, and the caller gives that
/// reference up.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_drop_release(p: *mut c_void, f: ReleaseFn) {
    if stack_ptr() < S.floor.load(Relaxed) + DEFER_SLACK {
        return unsafe { release_near_floor(p, f) };
    }
    unsafe { f(p) }
}

/// The slow leg of a release that found itself near or below the
/// floor: one out-of-line copy of the floor update and the deferral
/// loop, kept off the release's frameless fast path.
///
/// # Safety
/// As [`__torajs_drop_release`].
#[cold]
#[inline(never)]
#[optimize(size)]
pub(crate) unsafe fn release_near_floor(p: *mut c_void, f: ReleaseFn) {
    // one address for every field the loop reads and writes, instead
    // of a page address and an offset rebuilt at each of them
    let s = unsafe { &*core::hint::black_box(&raw const S) };
    if deep(s) {
        unsafe { run_deferred(s, p, f) };
    } else {
        unsafe { f(p) }
    }
}

/// Is the stack pointer within [`DEFER_SLACK`] above the floor, or
/// below it? Sets the provisional floor on the first test, and the
/// real one on the first test near that, so a false answer leaves the
/// floor more than [`DEFER_SLACK`] below the stack pointer.
#[inline(always)]
fn deep(s: &State) -> bool {
    let sp = stack_ptr();
    let mut lim = s.floor.load(Relaxed);
    if lim == UNSET {
        // a provisional floor carries its low bit set; real floors and
        // stack pointers are even
        s.floor.store((sp - PROVISIONAL_SLACK) | 1, Relaxed);
        return false;
    }
    if lim & 1 != 0 && sp < lim + DEFER_SLACK {
        lim = compute_floor();
        s.floor.store(lim, Relaxed);
    }
    sp < lim + DEFER_SLACK
}

#[inline(always)]
fn stack_ptr() -> usize {
    let sp: usize;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        core::arch::asm!("mov {}, sp", out(reg) sp, options(pure, nomem, nostack, preserves_flags));
    }
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) sp, options(pure, nomem, nostack, preserves_flags));
    }
    // no stack pointer to read: never below the floor
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        sp = 0;
    }
    sp
}

/// The main thread's stack bottom plus the margin. The JS runtime
/// releases on the main thread, whose stack the kernel sizes from
/// the stack limit and whose top the frame chain ends at. Both are
/// read without a libSystem import: every import is a bind that
/// every artifact pays for at launch.
#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
fn compute_floor() -> usize {
    // an unreadable limit leaves the floor at zero: nothing is deferred
    let Ok(size) = torajs_syscall::stack_limit() else {
        return 0;
    };
    let size = size as usize;
    (outermost_frame() + (size / 4).min(MARGIN_MAX)).saturating_sub(size)
}

/// Only arm64 Mach-O artifacts are linked; elsewhere nothing is
/// deferred.
#[cfg(not(all(target_vendor = "apple", target_arch = "aarch64")))]
fn compute_floor() -> usize {
    0
}

/// The outermost record on the calling thread's frame chain, a few
/// hundred bytes below the top of its stack. Each record's first
/// word is its caller's record, which lies above it; the chain ends
/// at zero.
#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
fn outermost_frame() -> usize {
    let mut fp: usize;
    unsafe {
        core::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack, preserves_flags));
    }
    loop {
        let next = unsafe { (fp as *const usize).read() };
        if next <= fp {
            return fp;
        }
        fp = next;
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Pending {
    p: *mut c_void,
    f: ReleaseFn,
}

/// Push the release, and run the pending stack unless an outer
/// deferral is already running it. While the loop runs, the floor sits
/// [`LOOP_SLACK`] below the loop, so a release it runs nests that far
/// at most before its own releases come back here.
#[inline(always)]
unsafe fn run_deferred(s: &State, p: *mut c_void, f: ReleaseFn) {
    push(s, Pending { p, f });
    if s.draining.load(Relaxed) {
        return;
    }
    s.draining.store(true, Relaxed);
    let floor = s.floor.load(Relaxed);
    s.floor.store(stack_ptr() - LOOP_SLACK, Relaxed);
    loop {
        let n = s.len.load(Relaxed);
        if n == 0 {
            break;
        }
        s.len.store(n - 1, Relaxed);
        // a release run below may grow (move) the stack: read it fresh
        let e = unsafe { *s.pending.load(Relaxed).add(n - 1) };
        unsafe { (e.f)(e.p) };
    }
    s.floor.store(floor, Relaxed);
    s.draining.store(false, Relaxed);
}

#[inline(always)]
fn push(s: &State, e: Pending) {
    let (len, cap) = (s.len.load(Relaxed), s.cap.load(Relaxed));
    if len == cap {
        let new_cap = if cap == 0 { 64 } else { cap * 2 };
        let sz = core::mem::size_of::<Pending>();
        let buf = unsafe {
            __torajs_realloc(
                s.pending.load(Relaxed) as *mut c_void,
                cap * sz,
                new_cap * sz,
            )
        };
        s.pending.store(buf as *mut Pending, Relaxed);
        s.cap.store(new_cap, Relaxed);
    }
    unsafe { s.pending.load(Relaxed).add(len).write(e) };
    s.len.store(len + 1, Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELLS: usize = 50_000;
    static RUNS: AtomicUsize = AtomicUsize::new(0);
    static CELL_BASE: AtomicUsize = AtomicUsize::new(0);
    /// the deepest stack pointer a release ran at
    static LOWEST: AtomicUsize = AtomicUsize::new(usize::MAX);
    /// the floor and the pending stack are process-wide
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// a cell is a header word whose low u32 is the count; cell `i`
    /// holds the only reference of cell `i - 1`. Tests the floor on
    /// entry, as a release that can nest does.
    unsafe extern "C" fn release_cell(p: *mut c_void) {
        if below_floor() {
            return unsafe { __torajs_drop_release(p, release_cell) };
        }
        let rc = p as *mut u32;
        unsafe {
            *rc -= 1;
            if *rc != 0 {
                return;
            }
        }
        LOWEST.fetch_min(stack_ptr(), Relaxed);
        RUNS.fetch_add(1, Relaxed);
        let base = CELL_BASE.load(Relaxed);
        let i = (p as usize - base) / 8;
        if i > 0 {
            unsafe { release_cell((base + (i - 1) * 8) as *mut c_void) };
        }
    }

    /// cell `n` releases cell `n - 1` through the release, as a class
    /// drop fn does along a back edge of its call graph
    unsafe extern "C" fn release_routed(p: *mut c_void) {
        let n = p as usize;
        LOWEST.fetch_min(stack_ptr(), Relaxed);
        RUNS.fetch_add(1, Relaxed);
        if n > 1 {
            unsafe { __torajs_drop_release((n - 1) as *mut c_void, release_routed) };
        }
    }

    fn reset() {
        RUNS.store(0, Relaxed);
        LOWEST.store(usize::MAX, Relaxed);
    }

    /// every release ran within the loop's slack (plus a few frames)
    /// below the test's own stack pointer
    fn assert_bounded(top: usize) {
        assert!(top - LOWEST.load(Relaxed) < LOOP_SLACK + (16 << 10));
        assert_eq!(S.len.load(Relaxed), 0);
        assert!(!S.draining.load(Relaxed));
    }

    #[test]
    fn deep_chain_releases_every_cell_once_at_bounded_depth() {
        let _serial = SERIAL.lock().unwrap();
        reset();
        let mut cells = std::vec![1u64; CELLS];
        CELL_BASE.store(cells.as_mut_ptr() as usize, Relaxed);
        // every release below the floor, as in a deep walk
        S.floor.store(UNSET - 1, Relaxed);
        let top = stack_ptr();
        unsafe { release_cell(cells.as_mut_ptr().add(CELLS - 1) as *mut c_void) };
        assert_eq!(S.floor.load(Relaxed), UNSET - 1);
        S.floor.store(UNSET, Relaxed);
        assert_eq!(RUNS.load(Relaxed), CELLS);
        assert!(cells.iter().all(|&c| c as u32 == 0));
        assert_bounded(top);
    }

    #[test]
    fn shared_cell_loses_one_reference_when_deferred() {
        let _serial = SERIAL.lock().unwrap();
        reset();
        let mut cells = std::vec![1u64, 2];
        CELL_BASE.store(cells.as_mut_ptr() as usize, Relaxed);
        S.floor.store(UNSET - 1, Relaxed);
        unsafe { release_cell(cells.as_mut_ptr().add(1) as *mut c_void) };
        S.floor.store(UNSET, Relaxed);
        assert_eq!((cells[0] as u32, cells[1] as u32), (1, 1));
        assert_eq!(RUNS.load(Relaxed), 0);
    }

    #[test]
    fn routed_chain_runs_every_release_at_bounded_depth() {
        let _serial = SERIAL.lock().unwrap();
        reset();
        S.floor.store(UNSET - 1, Relaxed);
        let top = stack_ptr();
        unsafe { __torajs_drop_release(100_000 as *mut c_void, release_routed) };
        S.floor.store(UNSET, Relaxed);
        assert_eq!(RUNS.load(Relaxed), 100_000);
        assert_bounded(top);
    }

    #[test]
    fn above_the_floor_a_release_runs_in_place() {
        let _serial = SERIAL.lock().unwrap();
        reset();
        S.floor.store(0, Relaxed);
        unsafe { __torajs_drop_release(3 as *mut c_void, release_routed) };
        S.floor.store(UNSET, Relaxed);
        assert_eq!(RUNS.load(Relaxed), 3);
        assert_eq!(S.len.load(Relaxed), 0);
    }

    #[test]
    fn floor_lies_inside_the_stack() {
        let lim = compute_floor();
        let here = stack_ptr();
        assert!(lim < here, "floor {lim:#x} above sp {here:#x}");
        assert!(here - lim < 16 << 20);
    }

    #[test]
    fn first_test_sets_a_provisional_floor_and_passing_it_the_real_one() {
        let _serial = SERIAL.lock().unwrap();
        S.floor.store(UNSET, Relaxed);
        assert!(!deep(&S));
        let provisional = S.floor.load(Relaxed);
        assert_eq!(provisional & 1, 1);
        assert!(provisional < stack_ptr());
        // a test below the provisional floor computes the real one
        S.floor.store((stack_ptr() + 4096) | 1, Relaxed);
        let real = compute_floor();
        assert_eq!(deep(&S), stack_ptr() < real + DEFER_SLACK);
        assert_eq!(S.floor.load(Relaxed), real);
        S.floor.store(UNSET, Relaxed);
    }

    #[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
    #[test]
    fn frame_chain_ends_at_the_top_of_the_stack() {
        unsafe extern "C" {
            fn pthread_self() -> *mut c_void;
            fn pthread_get_stackaddr_np(t: *mut c_void) -> *mut c_void;
        }
        let top = unsafe { pthread_get_stackaddr_np(pthread_self()) } as usize;
        let last = outermost_frame();
        assert!(
            stack_ptr() < last && last < top,
            "{last:#x} outside the stack below {top:#x}"
        );
        assert!(
            top - last < 64 << 10,
            "{last:#x} far below the top {top:#x}"
        );
    }
}
