//! How a bound cell is CALLED — §10.4.1.1 [[Call]].

use core::ffi::c_void;

use super::{
    BOUND_ARGC_OFF, BOUND_ARGS_OFF, BOUND_KIND_OFF, BOUND_TARGET_OFF, BOUND_THIS_OFF,
    bound_target_cell, unpack_builtin_target,
};
use crate::method_call::MAX_BOXED_ARGS;
use crate::nanbox::VALUE_UNDEFINED;

/// Boxed dual entry of a bound cell — concatenate the partial
/// arguments with the call's own and dispatch the target.
///
/// A bound target is called by §10.4.1.1 again: its own bound this
/// replaces this layer's, and its partials go in front of this
/// layer's. So the whole chain of bound layers resolves here in one
/// loop down to the first target that is not a bound cell, rather
/// than one native frame per layer, so the depth of the chain never
/// reaches the native stack.
pub(super) unsafe extern "C" fn bound_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let outer = env.cast::<u8>();
        let mut cell = outer;
        let mut bound_total = *(cell.add(BOUND_ARGC_OFF) as *const u64) as usize;
        while *(cell.add(BOUND_KIND_OFF) as *const u64) == 1 {
            let Some(next) = bound_target_cell(*(cell.add(BOUND_TARGET_OFF) as *const u64)) else {
                break;
            };
            bound_total += *(next.add(BOUND_ARGC_OFF) as *const u64) as usize;
            cell = next;
        }
        let kind = *(cell.add(BOUND_KIND_OFF) as *const u64);
        let target = *(cell.add(BOUND_TARGET_OFF) as *const u64);
        let bound_this = *(cell.add(BOUND_THIS_OFF) as *const u64);
        let cn = argc.max(0) as usize;
        let total = bound_total + cn;

        // Argument slots stay BORROWED end to end — the bound args
        // are the cells' own references, the tail is the caller's.
        // Layout: innermost layer's partials first, the outermost
        // layer's right before the call's own arguments.
        //
        // One fill and one dispatch site for both buffers keeps the
        // entry small: it is linked into every program that can reach
        // `bind`.
        let mut stack = [VALUE_UNDEFINED; MAX_BOXED_ARGS];
        let spill = core::alloc::Layout::from_size_align_unchecked(total * 8, 8);
        let buf = if total > MAX_BOXED_ARGS {
            let p = std::alloc::alloc(spill);
            if p.is_null() {
                std::alloc::handle_alloc_error(spill);
            }
            p as *mut u64
        } else {
            stack.as_mut_ptr()
        };
        core::ptr::copy_nonoverlapping(argv, buf.add(bound_total), cn);
        let mut at = bound_total;
        let mut layer = outer;
        loop {
            let bn = *(layer.add(BOUND_ARGC_OFF) as *const u64) as usize;
            at -= bn;
            core::ptr::copy_nonoverlapping(
                layer.add(BOUND_ARGS_OFF) as *const u64,
                buf.add(at),
                bn,
            );
            if layer == cell {
                break;
            }
            layer = *(layer.add(BOUND_TARGET_OFF) as *const u64) as *mut u8;
        }
        let n = total as i64;
        let ret = if kind == 0 {
            // Same target shape `.call` / `.apply` build, so the
            // family-generic gate runs identically on both paths.
            let (mid, fam) = unpack_builtin_target(target);
            crate::method_call_closure::dispatch(
                &crate::method_call_closure::CallTarget::Builtin(mid, fam),
                bound_this,
                buf,
                n,
            )
        } else {
            // §10.4.1.1 step 5 is `Call(target, boundThis, args)`
            // — the ordinary call, on whatever the target is. So
            // this is the explicit-`this` kernel and nothing
            // else: it holds all three of §7.2.3's callable
            // shapes, routes the bound this through the receiver
            // channel (a recv-first target takes it in argv[0],
            // RFC 20260717-objlit-anylane-recv knife 2d; a plain
            // closure drops it), and runs a callable Proxy's
            // `apply` trap. Spelling the dispatch out here named
            // the closure and the class and missed the proxy —
            // `p.bind(null, 5)()` returned undefined instead of
            // running the trap.
            crate::method_call_closure_dispatch::__torajs_any_call_with_this(
                target, bound_this, buf, n,
            )
        };
        if total > MAX_BOXED_ARGS {
            std::alloc::dealloc(buf as *mut u8, spill);
        }
        ret
    }
}
