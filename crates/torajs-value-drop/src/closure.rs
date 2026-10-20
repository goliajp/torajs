//! The `Tag::Closure` arm of [`crate::__torajs_value_drop_heap`], ahead
//! of its other arms: a closure is released far more often while it
//! stays alive (every call that passes one along) than any container,
//! so it asks the depth question only on the leg where it dies.

use core::ffi::c_void;

use torajs_rc::{__torajs_rc_dec, FLAG_SUBCLASSED, HeapHeader};

use super::{__torajs_cycle_buffer, __torajs_subclass_drop_entry, __torajs_value_drop_heap};

/// Byte offset of the synthesized drop-fn pointer in a Closure env
/// block — ABI mirror of `torajs-core::ssa_lower::CLOSURE_DROP_FN_OFF`
/// (`{ hdr | fn_ptr@8 | drop_fn@16 | props@24 | boxed_entry@32 |
/// trace_fn@40 | caps@48+ }`, see the `Tag::Closure` doc in
/// torajs-rc).
const CLOSURE_DROP_FN_OFF: usize = 16;

/// Byte offsets of the lazy expando bag and of the synthesized trace
/// pointer, from the same layout.
const CLOSURE_PROPS_OFF: usize = 24;
const CLOSURE_TRACE_FN_OFF: usize = 40;

/// Can a cycle run through this closure? The collector walks a
/// closure through its trace (0 when no capture slot can sit on a
/// cycle) and its expando bag, and `__torajs_cycle_buffer` asks the
/// same two words before it buffers — behind a frame and three gates
/// of its own. A function value passed along on every call is
/// released alive once per call, and most such values have neither,
/// so the two words are read here, before the call.
#[inline(always)]
unsafe fn may_close_ring(child: *mut c_void) -> bool {
    unsafe {
        let trace = (child.cast::<u8>().add(CLOSURE_TRACE_FN_OFF) as *const usize).read();
        let props = (child.cast::<u8>().add(CLOSURE_PROPS_OFF) as *const usize).read();
        trace != 0 || props != 0
    }
}

/// Release one reference on the closure env cell. On hit-zero, invoke
/// the Pass-2.5-synthesized `__env_drop_<closure>` whose address every
/// construction site stores at +16 (walks the props dynobj + Copy
/// capture boxes, then frees the env block) — the same indirect call
/// `emit_drop_closure` emits on the typed path. The synthesized body
/// has no rc gate of its own (typed sites own their single reference),
/// so the dec gate lives here.
///
/// # Safety
/// `child` is a live `Tag::Closure` cell; the caller gives up one
/// reference.
#[inline(always)]
pub(crate) unsafe fn release(child: *mut c_void) {
    unsafe {
        if __torajs_rc_dec(child) != 0 {
            // the env's captures may be a chain of closures: deep in
            // the stack the reference goes back and the release is
            // deferred whole. The count is written directly, not
            // through the rc helpers: while pending it reads 1, so a
            // cycle collection that runs first sees a live cell. The
            // deferred release decs it to zero a second time, which
            // fires the weak target-dying hook again for this cell;
            // that second call finds nothing, because the first one
            // removed the cell from the weak registry. Both steps
            // assume one mutator thread.
            if crate::dropguard::below_floor() {
                (child as *mut u32).write(1);
                return crate::dropguard::release_near_floor(child, __torajs_value_drop_heap);
            }
            // RFC 20260730 blade 0 — a Function-subclass instance
            // (closure cell minted through a user class) scrubs its
            // identity entry before the env drop frees it.
            if (*(child as *const HeapHeader)).flags & FLAG_SUBCLASSED != 0 {
                __torajs_subclass_drop_entry(child);
            }
            let drop_fn: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(
                (child.cast::<u8>().add(CLOSURE_DROP_FN_OFF) as *const usize).read(),
            );
            drop_fn(child);
        } else {
            // rc still positive — the exact cyclic-leak condition (RFC
            // 20260717 knife 3): buffer as a PURPLE cycle candidate,
            // mirroring the Obj and wrapper arms, unless no cycle can
            // pass through the env (cycle_buffer's own walkability
            // gate would turn it away, after the call)
            if may_close_ring(child) {
                __torajs_cycle_buffer(child);
            }
        }
    }
}
