//! How a bound cell is MADE — the allocation, and the thunk that
//! borrows the same shape. Split from `method_bind.rs` (file-size
//! HARD RULE) when [`callback_thunk`] pushed the parent past 500;
//! the parent keeps what `bind` DOES, this keeps what it builds.
//! Same-crate child module — every private parent item stays
//! reachable through `super::`.

use core::ffi::c_void;

use torajs_rc::Tag;

use super::{
    __torajs_anyv_box_pointer, BOUND_ARGC_OFF, BOUND_ARGS_OFF, BOUND_KIND_OFF, BOUND_LEN_OFF,
    BOUND_NAME_OFF, BOUND_NAME_PREFIXES_OFF, BOUND_TARGET_OFF, BOUND_THIS_OFF,
    CLOSURE_BOXED_ENTRY_OFF, CLOSURE_DROP_FN_OFF, CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF,
    CLOSURE_TRACE_FN_OFF, bound_drop, bound_entry, bound_layout, bound_trace, native_entry,
};
use crate::nanbox::AnyValue;

/// Allocate the bound cell. `len` / `name` are §20.2.3.2 steps
/// 5-14's already-resolved values, because both can run user code
/// and must have run BEFORE anything is allocated; `partials` are
/// the `n` bound arguments, borrowed.
///
/// [`callback_thunk`] mints through here too, with `len` 0 and
/// `name` NULL — it owes no `.length` or `.name` because it never
/// becomes a value the program can see.
///
/// # Safety
/// `target` is a live AnyValue the caller has already staked
/// (kind 1) or a packed builtin id (kind 0); `partials` points at
/// `n` borrowed AnyValue slots; `name.0` is NULL or an owned Str,
/// read with `name.1` more `bound ` in front of it.
pub(super) unsafe fn mint_bound(
    kind: u64,
    target: u64,
    this_arg: AnyValue,
    partials: *const u64,
    n: usize,
    len: f64,
    name: (*mut u8, u64),
) -> AnyValue {
    unsafe {
        let cell = std::alloc::alloc_zeroed(bound_layout(n));
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = 0;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = bound_drop as *const () as u64;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = bound_entry as *const () as u64;
        *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = bound_trace as *const () as u64;
        *(cell.add(BOUND_KIND_OFF) as *mut u64) = kind;
        *(cell.add(BOUND_TARGET_OFF) as *mut u64) = target;
        // The cell takes its own stake on the captured values
        // (NaN-box-safe: immediates no-op).
        crate::nanbox_ffi::__torajs_anyv_rc_inc(this_arg);
        *(cell.add(BOUND_THIS_OFF) as *mut u64) = this_arg;
        *(cell.add(BOUND_ARGC_OFF) as *mut u64) = n as u64;
        *(cell.add(BOUND_LEN_OFF) as *mut f64) = len;
        *(cell.add(BOUND_NAME_OFF) as *mut u64) = name.0 as u64;
        *(cell.add(BOUND_NAME_PREFIXES_OFF) as *mut u64) = name.1;
        for i in 0..n {
            let av = *partials.add(i);
            crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
            *(cell.add(BOUND_ARGS_OFF + i * 8) as *mut u64) = av;
        }
        __torajs_anyv_box_pointer(cell as *mut c_void)
    }
}

/// A cell that carries `av` and `this_arg` behind the ordinary
/// boxed dual ABI, so a loop that can only call `(env, entry)` can
/// call a value that has no entry of its own.
///
/// The bound cell already IS that shape — §10.4.1.1 step 5 is the
/// ordinary call on whatever the target is — so a thunk is a bound
/// cell with no partial arguments and no `.length` / `.name`. It
/// never becomes a value the program can reach: the gate mints it,
/// hands the pair to the loop, and drops it when the loop returns.
/// So §20.2.3.2 steps 5-14 are skipped, which also keeps their two
/// Gets off a proxy target — reads the spec does not ask for here.
///
/// `None` when `av` is not callable, which is the caller's own
/// TypeError to phrase.
///
/// # Safety
/// `av` and `this_arg` are valid AnyValues, borrowed.
pub(crate) unsafe fn callback_thunk(av: AnyValue, this_arg: AnyValue) -> Option<AnyValue> {
    unsafe {
        if !crate::construct::is_callable(av) {
            return None;
        }
        crate::nanbox_ffi::__torajs_anyv_rc_inc(av);
        Some(mint_bound(
            1,
            av,
            this_arg,
            core::ptr::null(),
            0,
            0.0,
            (core::ptr::null_mut(), 0),
        ))
    }
}
