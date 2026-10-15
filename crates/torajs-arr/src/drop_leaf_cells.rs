//! One call for the last-owner release of an array whose elements are
//! instances of a struct with no reference fields (`Token[]` for a
//! `class Token { kind: number; value: number }`).
//!
//! Lowering used to emit that walk into the user's code: per slot a
//! nullish test, the refcount decrement, and on a dying cell the
//! WeakRef observer test, the expando-bag test, the cycle-buffer flag
//! test and the sized free — about 34 instructions per element, eight
//! of them the nullish test spelled as two compares and an `or`, and
//! four the loop bookkeeping around an alloca'd counter. Here the same
//! tests run in one compiled loop with the loop-invariant parts
//! (sentinel address, the WeakRef observer count) held in registers,
//! and the allocator row the dead cells go onto held there too: lifted
//! out of the cache for the walk and put back around any call out.
//!
//! Every element keeps the release the inline sequence gave it: a cell
//! still shared after the decrement is only decremented (a class
//! instance carrying an expando bag is offered to the cycle buffer
//! through the bag's seam), and a dying cell tells live WeakRef
//! observers, releases its bag, stays allocated while the cycle
//! collector holds it as a candidate root, and is freed otherwise.
//! A cell whose slot can hold a wider layout, or one `any` stored
//! into, is freed by what the cell says of itself (`*_by_cell`), as
//! `cell_size::settle` decides for the inline free.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_mmalloc::core::__torajs_core_tlab;
use torajs_mmalloc::extern_api::__torajs_free;
use torajs_mmalloc::size_class::bucket_for;
use torajs_mmalloc::tlab::LiftedRow;
use torajs_rc::undef_cell::is_undef_cell;
use torajs_rc::{COLOR_MASK, FLAG_BUFFERED, HeapHeader};

use crate::drop::__torajs_arr_drop;
use crate::layout::{ARR_CAP_OFF, arr_data, arr_live_extent};

/// The expando-bag pointer of a struct cell (`OBJ_PROPS_OFF` in
/// torajs-core `ssa_lower.rs`).
const OBJ_PROPS_OFF: usize = 24;

unsafe extern "C" {
    /// torajs-rc — live weak-observer count, one word across archives.
    static __torajs_weakref_active: AtomicU64;
    /// torajs-weak — clear the observers of a dying `target`.
    fn __torajs_weakref_target_dying(target: *mut c_void);
    /// Link seam — release the expando bag of a dying struct cell.
    fn __torajs_obj_props_drop_slow(cell: *mut c_void);
    /// Link seam — buffer a still-shared fields-all-scalar instance
    /// whose bag could close a cycle.
    fn __torajs_obj_buffer_slow(cell: *mut c_void);
    /// torajs-cycle — free a cell by its own tag or class row.
    fn __torajs_obj_free_cell(p: *mut c_void);
}

/// Where dying elements go, and how a call out of the walk keeps the
/// allocator consistent.
trait Sink {
    /// Run `f`, which may allocate or free.
    unsafe fn call_out(&mut self, f: impl FnOnce());
    /// Hand the dead cell `elem` back.
    unsafe fn free(&mut self, elem: *mut u8);
    /// The walk is over.
    fn finish(self);
}

/// Dead cells onto their size class's allocator row, which the walk
/// holds in registers: lifted out of the cache, and put back around
/// every call out.
struct RowSink {
    size: usize,
    row: LiftedRow,
}

impl Sink for RowSink {
    #[inline(always)]
    unsafe fn call_out(&mut self, f: impl FnOnce()) {
        let tlab = &raw mut __torajs_core_tlab;
        unsafe { (*tlab).lower(&self.row) };
        f();
        self.row = unsafe { (*tlab).relift(&self.row) };
    }

    #[inline(always)]
    unsafe fn free(&mut self, elem: *mut u8) {
        if !unsafe { self.row.push(elem) } {
            // a full row spills inside the allocator
            let size = self.size;
            unsafe { self.call_out(|| __torajs_free(elem as *mut c_void, size)) };
        }
    }

    #[inline(always)]
    fn finish(self) {
        unsafe { (*(&raw mut __torajs_core_tlab)).lower(&self.row) };
    }
}

/// Dead cells through the allocator: a size past the small classes,
/// a census build (the allocator counts every free), or `None`, each
/// cell by its own tag or class row.
struct CallSink {
    size: Option<usize>,
}

impl Sink for CallSink {
    #[inline(always)]
    unsafe fn call_out(&mut self, f: impl FnOnce()) {
        f();
    }

    #[inline(always)]
    unsafe fn free(&mut self, elem: *mut u8) {
        match self.size {
            Some(size) => unsafe { __torajs_free(elem as *mut c_void, size) },
            None => unsafe { __torajs_obj_free_cell(elem as *mut c_void) },
        }
    }

    #[inline(always)]
    fn finish(self) {}
}

/// Release the slots of `p` when this is its last owner, then the
/// array itself. `CLASS` is true for a class's instances: they carry
/// the WeakRef notice, the cycle-buffer flag and the shared-release
/// buffer seam, which an anonymous struct cell never needs. `size`:
/// the block size of every cell the slots can hold, `None` to free
/// each by what it says of itself.
///
/// # Safety
/// `p` is a live array cell whose slots hold null, the `undefined`
/// cell, or struct cells with no reference fields, of `size` bytes
/// when it is given.
#[inline(always)]
unsafe fn release_leaf_array<const CLASS: bool>(p: *mut c_void, size: Option<usize>) {
    let cell = p as *mut u8;
    // only the last owner walks: the arr_drop below is then the
    // hit-zero release of the block
    if unsafe { (*(cell as *const HeapHeader)).refcount } == 1 {
        let extent = unsafe { arr_live_extent(cell) } as usize;
        // cap:u32 | head:u32 packed at ARR_CAP_OFF (deque head)
        let head = unsafe { *(cell.add(ARR_CAP_OFF + 4) as *const u32) } as usize;
        let slots = unsafe { arr_data(cell).add(head * 8) as *const *mut u8 };
        let row = match size {
            Some(n) if !cfg!(torajs_census) => {
                bucket_for(n).and_then(|c| unsafe { (*(&raw const __torajs_core_tlab)).lift(c) })
            }
            _ => None,
        };
        match (size, row) {
            (Some(size), Some(row)) => unsafe {
                walk::<CLASS, _>(slots, extent, RowSink { size, row })
            },
            (size, _) => unsafe { walk::<CLASS, _>(slots, extent, CallSink { size }) },
        }
    }
    unsafe { __torajs_arr_drop(p) };
}

/// Release every cell in `slots[..extent]` into `sink`.
///
/// # Safety
/// As [`release_leaf_array`], for the slots it read.
#[inline(always)]
unsafe fn walk<const CLASS: bool, S: Sink>(slots: *const *mut u8, extent: usize, mut sink: S) {
    // read once: releasing cells runs no user code, and only user
    // code registers a weak observer; one that goes away meanwhile
    // costs a notice that finds nobody
    let observed = CLASS && unsafe { __torajs_weakref_active.load(Ordering::Relaxed) } != 0;
    let mut slot = slots;
    let end = unsafe { slots.add(extent) };
    while slot != end {
        let elem = unsafe { *slot };
        slot = unsafe { slot.add(1) };
        if elem.is_null() || is_undef_cell(elem) {
            continue;
        }
        unsafe { release_leaf::<CLASS, S>(elem, &mut sink, observed) };
    }
    sink.finish();
}

/// One reference of the struct cell `elem`, in the order the inline
/// drop makes them: decrement; a shared class instance with a bag is
/// buffered; a dying one notifies observers, releases its bag, and is
/// freed unless the cycle collector holds it. `observed`: some weak
/// observer may be alive.
///
/// # Safety
/// `elem` is a live struct cell with no reference fields.
#[inline(always)]
unsafe fn release_leaf<const CLASS: bool, S: Sink>(elem: *mut u8, sink: &mut S, observed: bool) {
    let hdr = elem as *mut HeapHeader;
    let rc = unsafe { (*hdr).refcount };
    debug_assert!(rc != 0, "release of a cell at refcount 0");
    unsafe { (*hdr).refcount = rc - 1 };
    let props = elem.wrapping_add(OBJ_PROPS_OFF) as *const *mut c_void;
    let arg = elem as *mut c_void;
    if rc != 1 {
        if CLASS && !unsafe { *props }.is_null() {
            unsafe { sink.call_out(|| __torajs_obj_buffer_slow(arg)) };
        }
        return;
    }
    if CLASS && observed {
        unsafe { sink.call_out(|| __torajs_weakref_target_dying(arg)) };
    }
    if !unsafe { *props }.is_null() {
        unsafe { sink.call_out(|| __torajs_obj_props_drop_slow(arg)) };
    }
    if CLASS && unsafe { (*hdr).flags } & FLAG_BUFFERED != 0 {
        // a candidate root stays allocated, coloured black at rc 0:
        // the next collect's root walk frees it
        unsafe { (*hdr).flags &= !COLOR_MASK };
        return;
    }
    unsafe { sink.free(elem) };
}

/// Release an array of anonymous struct cells of `size` bytes.
///
/// # Safety
/// As [`release_leaf_array`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_leaf_objs(p: *mut c_void, size: usize) {
    unsafe { release_leaf_array::<false>(p, Some(size)) }
}

/// Release an array of anonymous struct cells, each freed by its tag.
///
/// # Safety
/// As [`release_leaf_array`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_leaf_objs_by_cell(p: *mut c_void) {
    unsafe { release_leaf_array::<false>(p, None) }
}

/// Release an array of class instances of `size` bytes.
///
/// # Safety
/// As [`release_leaf_array`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_leaf_instances(p: *mut c_void, size: usize) {
    unsafe { release_leaf_array::<true>(p, Some(size)) }
}

/// Release an array of class instances, each freed by its class row.
///
/// # Safety
/// As [`release_leaf_array`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_drop_leaf_instances_by_cell(p: *mut c_void) {
    unsafe { release_leaf_array::<true>(p, None) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alloc::__torajs_arr_alloc;
    use crate::layout::ARR_LEN_OFF;
    use torajs_mmalloc::core::alloc_sized;
    use torajs_rc::undef_cell::undef_cell_ptr;

    const CELL: usize = 48;

    fn cell(rc: u32) -> *mut u8 {
        let p = alloc_sized(CELL);
        unsafe {
            core::ptr::write_bytes(p, 0, CELL);
            (*(p as *mut HeapHeader)).refcount = rc;
        }
        p
    }

    fn array(slots: &[*mut u8]) -> *mut c_void {
        unsafe {
            let a = __torajs_arr_alloc(slots.len() as u64);
            for (i, s) in slots.iter().enumerate() {
                *(arr_data(a).add(i * 8) as *mut *mut u8) = *s;
            }
            *(a.add(ARR_LEN_OFF) as *mut u64) = slots.len() as u64;
            a as *mut c_void
        }
    }

    #[test]
    fn last_owner_frees_dying_cells_and_decrements_shared_ones() {
        let dying = cell(1);
        let shared = cell(2);
        let a = array(&[dying, core::ptr::null_mut(), shared, undef_cell_ptr()]);
        unsafe { __torajs_arr_drop_leaf_instances(a, CELL) };
        assert_eq!(unsafe { (*(shared as *const HeapHeader)).refcount }, 1);
        // the allocator row is LIFO: the freed cell comes straight back
        assert_eq!(alloc_sized(CELL), dying);
    }

    #[test]
    fn a_shared_array_leaves_its_slots_alone() {
        let elem = cell(1);
        let a = array(&[elem]);
        unsafe { (*(a as *mut HeapHeader)).refcount = 2 };
        unsafe { __torajs_arr_drop_leaf_objs(a, CELL) };
        assert_eq!(unsafe { (*(elem as *const HeapHeader)).refcount }, 1);
        assert_eq!(unsafe { (*(a as *const HeapHeader)).refcount }, 1);
    }

    #[test]
    fn a_buffered_instance_stays_allocated_and_turns_black() {
        let elem = cell(1);
        unsafe { (*(elem as *mut HeapHeader)).flags = FLAG_BUFFERED | COLOR_MASK };
        let a = array(&[elem]);
        unsafe { __torajs_arr_drop_leaf_instances(a, CELL) };
        let hdr = unsafe { &*(elem as *const HeapHeader) };
        assert_eq!((hdr.refcount, hdr.flags), (0, FLAG_BUFFERED));
    }
}
