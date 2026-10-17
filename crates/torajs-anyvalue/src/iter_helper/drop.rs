//! Release of an IterHelper cell.

use core::ffi::c_void;

use super::{CELL_SIZE, FN_OFF, INNER_OFF, NEXT_OFF, PROPS_OFF, UNDERLYING_OFF};

/// Drop glue — the torajs-value-drop dispatcher contract is
/// RELEASE ONE REFERENCE: rc-dec the cell itself, and only on
/// hit-zero release the four owned AnyValue slots and free the
/// block (mirror of `__torajs_arr_iter_drop`). The 刀 2b churn
/// caught the unconditional-free first cut: a chained helper's
/// underlying release freed the inner cell while its binding still
/// held a reference (double-free SIGTRAP within hundreds of
/// iterations; single-layer cells survived by accident — their one
/// release WAS the last).
///
/// # Safety
/// `cell` is a live IterHelper cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_iter_helper_drop(cell: *mut c_void) {
    unsafe {
        if torajs_rc::__torajs_rc_dec(cell) == 0 {
            // Still referenced. A live own-property bag makes this cell a
            // potential cycle root — the shape rotation 528 taught the
            // collector to walk, and the reason it can now be reached.
            crate::__torajs_cycle_buffer(cell);
            return;
        }
        // Scrub from the root buffer before the memory goes away: a
        // cell buffered above that later normal-drops to zero would
        // leave a dangling candidate. Done before the slots go, so a
        // chain released newest first meets each cell at the
        // buffer's tail. No-op when never buffered.
        crate::__torajs_cycle_unbuffer(cell);
        let p = cell.cast::<u8>();
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(UNDERLYING_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(FN_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(INNER_OFF) as *const u64).read());
        crate::nanbox_ffi::__torajs_anyv_rc_dec((p.add(NEXT_OFF) as *const u64).read());
        // Own-property bag — a raw cell pointer, not an AnyValue, so
        // it takes the universal dispatcher rather than the nan-box
        // release above.
        let props = (p.add(PROPS_OFF) as *const u64).read() as *mut c_void;
        if !props.is_null() {
            (p.add(PROPS_OFF) as *mut u64).write(0);
            crate::__torajs_value_drop_heap(props);
        }
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(p, layout);
    }
}
