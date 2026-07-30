//! Function-subclass instance mint (RFC
//! 20260730-exotic-backed-class-instance blade 2).
//!
//! `class C extends Function` mints a REAL `Tag::Closure` cell —
//! `typeof` answers "function", `instanceof Function` rides tag-eq,
//! and the instance is callable through the ordinary boxed-dual lane.
//! `super()` contributes nothing beyond the mint (§20.2.1.1 with no
//! body source is the empty function: callable, ignores its
//! arguments, answers undefined); the body-source form
//! (`super("a", "return a")`) requires dynamic compilation and stays
//! a loud desugar boundary (the eval-shape RFC's seam).
//!
//! The cell mirrors the interned method-cell layout
//! (`method_value.rs`) but is a REAL rc'd instance: refcount 1, no
//! static flag, `FLAG_SUBCLASSED` + a blade-0 side-table entry
//! (scrubbed by the value-drop dispatcher's Closure arm), and a drop
//! fn mirroring the synthesized `__env_drop_*` shape — cycle
//! unbuffer, props release, free.

use core::ffi::c_void;

use torajs_rc::{FLAG_SUBCLASSED, Tag};

use crate::nanbox::VALUE_UNDEFINED;

/// Closure layout mirrors (`ssa_lower.rs` CLOSURE_* / tag.rs doc).
const CLOSURE_FN_ADDR_OFF: usize = 8;
const CLOSURE_DROP_FN_OFF: usize = 16;
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_BOXED_ENTRY_OFF: usize = 32;
/// Header + the five fixed slots + one (zero) capture word — the
/// same block the interned method cells carry.
const CELL_SIZE: usize = 56;

unsafe extern "C" {
    /// torajs-meta — record the fresh instance's class identity
    /// (blade 0). Takes no reference on the proto cell.
    fn __torajs_subclass_register(cell: *mut c_void, class_tag: i64, proto_cell: u64);
    /// torajs-meta classmeta — the class's registered `__proto_<C>`
    /// AnyValue immediate (0 when unregistered).
    fn __torajs_proto_cell_raw(tag: i64) -> u64;
    /// Universal drop dispatcher (release the props dict on drop).
    fn __torajs_value_drop_heap(child: *mut c_void);
    /// torajs-cycle — scrub a dying block from the root buffer.
    fn __torajs_cycle_unbuffer(p: *mut c_void);
}

/// Boxed dual entry — the empty function: ignore the arguments,
/// answer undefined (§20.2.1.1 with no body source).
unsafe extern "C" fn empty_fn_entry(_env: *mut c_void, _argv: *const u64, _argc: i64) -> u64 {
    VALUE_UNDEFINED
}

/// Typed fn-addr slot stand-in — a subclass instance is any-typed,
/// so every legitimate call rides the boxed dual; a typed-slot cast
/// direct call answers 0 harmlessly (same posture as the interned
/// method cells' loud native entry, minus the throw: calling the
/// instance IS legal).
unsafe extern "C" fn noop_fn_addr() -> u64 {
    0
}

/// Drop fn stored at `+16` — the value-drop dispatcher's Closure arm
/// calls it unconditionally on hit-zero (after the flag-gated
/// side-table scrub). Mirrors the synthesized `__env_drop_*` shape.
unsafe extern "C" fn function_subclass_env_drop(p: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(p);
        let props = (p.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const *mut c_void).read();
        if !props.is_null() {
            __torajs_value_drop_heap(props);
        }
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        std::alloc::dealloc(p as *mut u8, layout);
    }
}

/// Mint a Function-subclass instance: a fresh callable empty-function
/// closure cell carrying the class identity. Answers the boxed
/// instance — subclass instances live in the any world.
///
/// # Safety
/// `class_tag` is the class's registered tag.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_function_subclass_alloc(class_tag: i64) -> u64 {
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_SUBCLASSED;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = noop_fn_addr as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) =
            function_subclass_env_drop as *const () as u64;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = empty_fn_entry as *const () as u64;
        let proto_cell = __torajs_proto_cell_raw(class_tag);
        __torajs_subclass_register(cell as *mut c_void, class_tag, proto_cell);
        crate::nanbox::box_void_ptr(cell as *mut c_void)
    }
}
