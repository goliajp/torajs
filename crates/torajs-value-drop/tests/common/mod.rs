//! Shared by the async frame tests: link stand-ins for the runtime
//! crates these test binaries do not link, and a frame the way a
//! compiled async function lays one out, with a drop and a trace of
//! the shape the compiler synthesizes for a frame with no live slots.

#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering::Relaxed};

use torajs_promise::frame::{
    __torajs_frame_release_links, __torajs_frame_result_alloc, AWAITING_PARKED, AsyncFrame,
    FRAME_HEADER_SIZE, FrameDesc, TAG_ASYNC_FRAME,
};
use torajs_promise::layout::REPR_I64;
use torajs_promise::reaction::record::TraceVisit;
use torajs_rc::{COLOR_MASK, FLAG_BUFFERED, HeapHeader};

// Per-tag drops and hooks of crates these binaries do not link: no
// test here makes a cell of their shapes, so each panics if reached.
macro_rules! unreachable_stub {
    ($($name:ident),+ $(,)?) => {$(
        /// # Safety
        /// Never called; any call panics.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $name(_p: *mut c_void) {
            panic!(concat!(stringify!($name), " reached from the async frame tests"));
        }
    )+};
}

unreachable_stub!(
    __torajs_str_drop,
    __torajs_arr_drop,
    __torajs_arr_drop_any,
    __torajs_arr_drop_heap,
    __torajs_bigint_drop,
    __torajs_weakref_drop,
    __torajs_weakmap_drop,
    __torajs_weakset_drop,
    __torajs_finreg_drop,
    __torajs_map_drop,
    __torajs_set_drop,
    __torajs_map_iter_drop,
    __torajs_arr_iter_drop,
    __torajs_iter_helper_drop,
    __torajs_proxy_drop,
    __torajs_arraybuffer_drop,
    __torajs_typedarray_drop,
    __torajs_dataview_drop,
    __torajs_dynobj_drop,
    __torajs_accessor_drop,
    __torajs_global_cell_drop,
    __torajs_response_drop,
    __torajs_regex_drop,
    __torajs_date_drop,
    __torajs_symbol_drop,
    __torajs_number_wrapper_drop,
    __torajs_string_wrapper_drop,
    __torajs_boolean_wrapper_drop,
    __torajs_symbol_wrapper_drop,
    __torajs_subclass_drop_entry,
);

/// No weak target dies here.
///
/// # Safety
/// Accepts any pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut c_void) {}

/// No throw is ever pending.
#[unsafe(no_mangle)]
pub static __torajs_throw_active: AtomicI64 = AtomicI64::new(0);

/// No class is declared: the collector's class-instance rows, which a
/// compiled program's link emits.
#[unsafe(no_mangle)]
pub static __torajs_n_class_layouts: u32 = 0;
#[unsafe(no_mangle)]
pub static __torajs_class_layouts: torajs_cycle::layout::ClassLayout =
    torajs_cycle::layout::ClassLayout {
        n_children: 0,
        flags: 0,
        child_offsets: ptr::null(),
        field_metadata_ptr: ptr::null(),
        method_table_ptr: ptr::null(),
    };

/// A frame with no parameters and no slots.
pub const FRAME_SIZE: usize = FRAME_HEADER_SIZE;

/// Runs of [`drop_frame`].
pub static DROPS: AtomicUsize = AtomicUsize::new(0);

pub static DESC: FrameDesc = FrameDesc {
    resume: no_resume,
    drop: drop_frame,
    trace: trace_frame,
};

unsafe extern "C" {
    fn __torajs_malloc(size: usize) -> *mut c_void;
    fn __torajs_free(p: *mut c_void, size: usize);
}

unsafe extern "C" fn no_resume(_frame: i64) {
    unreachable!("nothing settles what these frames wait on");
}

/// Reports `result` and an owned `awaiting`, each with its word.
unsafe extern "C" fn trace_frame(p: *mut c_void, visit: TraceVisit, ctx: *mut c_void) {
    let f = p as *mut AsyncFrame;
    unsafe {
        let result = &raw mut (*f).result;
        visit(0, *result, result as *mut c_void, ctx);
        let awaiting = &raw mut (*f).awaiting;
        if *awaiting as usize & AWAITING_PARKED == 0 {
            visit(1, *awaiting, awaiting as *mut c_void, ctx);
        }
    }
}

/// A frame's drop: release its links, then free the block, or, while
/// the cycle collector holds it as a candidate root, leave it black
/// with its size over `desc` for the collector to free.
unsafe extern "C" fn drop_frame(p: *mut c_void) {
    DROPS.fetch_add(1, Relaxed);
    unsafe {
        __torajs_frame_release_links(p);
        let h = p as *mut HeapHeader;
        if (*h).flags & FLAG_BUFFERED != 0 {
            *(p as *mut usize).add(1) = FRAME_SIZE;
            (*h).flags &= !COLOR_MASK;
            return;
        }
        __torajs_free(p, FRAME_SIZE);
    }
}

/// A fresh frame, parked at its first suspension point, holding a
/// pending result; the caller owns its one reference.
pub fn new_frame() -> *mut AsyncFrame {
    unsafe {
        let f = __torajs_malloc(FRAME_SIZE) as *mut AsyncFrame;
        f.write(AsyncFrame {
            header: torajs_promise::layout::HeapHeader {
                refcount: 1,
                type_tag: TAG_ASYNC_FRAME,
                flags: 0,
            },
            desc: &DESC,
            next: ptr::null_mut(),
            awaiting: ptr::null_mut(),
            result: __torajs_frame_result_alloc(REPR_I64 as i64),
            state: 1,
            direct: 0,
        });
        f
    }
}

pub fn drops() -> usize {
    DROPS.load(Relaxed)
}

/// A block of the frame's size class, fresh from the allocator.
pub fn malloc_frame_sized() -> *mut c_void {
    unsafe { __torajs_malloc(FRAME_SIZE) }
}
