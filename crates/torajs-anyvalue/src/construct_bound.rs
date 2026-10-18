//! §10.4.1.2 [[Construct]] of a bound function, split out of
//! [`crate::construct`] under the 500-line file cap: `construct`
//! decides which shape a callee is, this file unwinds a chain of
//! bound layers in front of whatever shape sits at the bottom.

use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_void_ptr};
use crate::nanbox_encode::__torajs_anyv_box_pointer;
use core::ffi::c_void;

/// §10.4.1.2 [[Construct]] of a kind-1 bound cell, the whole chain
/// at once. A bound target is constructed by the same rule again: its
/// partials go in front of this layer's, and step 4 swaps a newTarget
/// that is SameValue to the layer for the layer's target. So the chain
/// resolves in one loop down to the first target that is not a bound
/// cell, rather than one native frame per layer, and the depth of the
/// chain never reaches the native stack. The is_constructor gate of
/// the caller already proved the chain bottoms out at a constructor.
///
/// # Safety
/// `callee` is a live kind-1 bound cell; `argv` points at `argc`
/// readable AnyValue slots; `new_target` is a live AnyValue.
pub(crate) unsafe fn construct_bound(
    callee: AnyValue,
    argv: *const u64,
    argc: i64,
    new_target: AnyValue,
) -> AnyValue {
    unsafe {
        let (bottom, nt, bound_total) = resolve_bound(callee, new_target);
        // Innermost layer's partials first, the outermost layer's
        // right before the call's own arguments — borrowed slots.
        let cn = argc.max(0) as usize;
        let mut merged = vec![VALUE_UNDEFINED; bound_total + cn];
        core::ptr::copy_nonoverlapping(argv, merged.as_mut_ptr().add(bound_total), cn);
        let mut at = bound_total;
        let mut layer = callee;
        while layer != bottom {
            let cell = as_void_ptr(layer);
            let Some((_, target, bn)) = crate::method_bind::bound_cell_meta(cell) else {
                break;
            };
            at -= bn;
            core::ptr::copy_nonoverlapping(
                crate::method_bind::bound_args_ptr(cell),
                merged.as_mut_ptr().add(at),
                bn,
            );
            layer = __torajs_anyv_box_pointer(target as *mut c_void);
        }
        crate::construct::construct_with_nt(bottom, merged.as_ptr(), merged.len() as i64, nt)
    }
}

/// The first target down `callee`'s bound chain that is not a bound
/// cell, the newTarget it is constructed with (each layer swaps a
/// newTarget SameValue to itself for its target, §10.4.1.2 step 4),
/// and the number of partials the layers carry. `callee` itself when
/// it is not bound.
///
/// # Safety
/// `callee` and `new_target` are live AnyValues; `callee` is a cell.
pub(crate) unsafe fn resolve_bound(
    callee: AnyValue,
    new_target: AnyValue,
) -> (AnyValue, AnyValue, usize) {
    unsafe {
        let mut nt = new_target;
        let mut layer = callee;
        let mut bound_total = 0usize;
        loop {
            let Some((1, target, bn)) = bound_closure_meta(as_void_ptr(layer)) else {
                return (layer, nt, bound_total);
            };
            bound_total += bn;
            let target_av = __torajs_anyv_box_pointer(target as *mut c_void);
            if nt == layer {
                nt = target_av;
            }
            layer = target_av;
        }
    }
}

/// [`crate::method_bind::bound_cell_meta`] of any live heap cell — a
/// class object or a Proxy is a different cell shape, and that one may
/// only look at a closure. A kind-1 bound target is such a cell.
///
/// # Safety
/// `p` is a live heap cell.
pub(crate) unsafe fn bound_closure_meta(p: *mut c_void) -> Option<(u64, u64, usize)> {
    unsafe {
        if (p.cast::<u8>().add(4) as *const u16).read() != torajs_rc::Tag::Closure as u16 {
            return None;
        }
        crate::method_bind::bound_cell_meta(p)
    }
}
