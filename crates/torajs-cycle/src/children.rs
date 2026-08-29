//! Per-shape child enumeration and slot clearing — the layout half
//! of the collector.
//!
//! [`collect`](crate::collect) owns the three Bacon-Rajan phases:
//! which work runs, and when. This file owns the one question those
//! phases keep asking of every cell they reach — *where does this
//! shape keep its children, and how is one of those slots cleared?*
//! Every cyclic shape's layout knowledge lives here and nowhere
//! else, so adding a shape to the walk is one arm in each of the two
//! functions below, not a change to the algorithm.
//!
//! The two are a pair: the slot index `i` that [`for_each_child`]
//! hands to its callback is the same `i` [`clear_child_slot`] takes
//! back, and only these two agree on what it means.

use core::ffi::c_void;

use crate::arr::{ARR_PROPS_OFF, arr_child_at, arr_len_of, arr_slot_clear};
use crate::closure_walk::{closure_trace_fn, trace_clear_tramp, trace_visit_tramp};
use crate::dynobj::{dynobj_child_at, dynobj_entries_len, dynobj_value_slot_clear};
use crate::layout::{
    CLOSURE_PROPS_OFF, HeapHeader, OBJ_PROPS_OFF, TAG_BOOLEAN_WRAPPER, TAG_CLOSURE, TAG_DYNOBJ,
    TAG_NUMBER_WRAPPER, TAG_STRING_WRAPPER, TAG_SYMBOL_WRAPPER, WRAPPER_PROPS_OFF,
    arr_elems_walkable, is_class_obj, layout_for_class_obj, nan_box_is_cell_like,
};
use crate::layout_bag::{TAG_MAP, TAG_SET};

/// Sentinel child index for a cell's out-of-band expando / props
/// slot (Arr +24, wrapper +16) — element indices are dense from 0,
/// so MAX can't collide.
const PROPS_SLOT_INDEX: u64 = u64::MAX;

/// Per-shape walkable-child slot enumeration — the ONLY place that
/// knows how each cyclic shape stores its children (class Obj =
/// layout child_offsets; Arr = element slots). Calls `f(i, child)`
/// for every non-null candidate slot; callers apply their own
/// `has_walkable_children` / color / rc gates. The slot index `i`
/// round-trips into [`clear_child_slot`].
///
/// # Safety
/// `p` must satisfy `has_walkable_children`. The walk reads class
/// layouts / arr slots — caller must guarantee the heap is
/// well-formed.
pub(crate) unsafe fn for_each_child(p: *mut c_void, mut f: impl FnMut(u64, *mut c_void)) {
    if unsafe { is_class_obj(p) } {
        let lay = unsafe { layout_for_class_obj(p) };
        let n_children = unsafe { (*lay).n_children };
        for i in 0..n_children as u64 {
            let off = unsafe { *(*lay).child_offsets.add(i as usize) };
            let child = unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) };
            // An `any`-typed field holds NaN-box bits, not a pointer,
            // whenever it carries an immediate — `undefined` is
            // `0x…0a`, and dereferencing that reads address 10. Every
            // other shape filters immediates where it produces them
            // (`dynobj_child_at`, `arr_child_at`, the closure trace
            // trampoline); this arm answered them raw, so a consumer
            // without its own gate crashed. `collect_white`'s first
            // sweep has one (`has_walkable_children`) and its second
            // does not, which is where it landed.
            if !child.is_null() && nan_box_is_cell_like(child) {
                f(i, child);
            }
        }
        // +24 expando props dict (RFC 20260714-struct-dynamic-props
        // blade 1) — same fixed slot Closure / Arr / wrapper carry.
        // An `obj.x = <cycle>` expando is a cycle child like any
        // layout field.
        let props = unsafe { *((p as *const u8).add(OBJ_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_DYNOBJ {
        // Dense entry values (holes / NaN-box immediates filter to
        // NULL inside dynobj_child_at).
        let n = unsafe { dynobj_entries_len(p) };
        for i in 0..n {
            let child = unsafe { dynobj_child_at(p, i) };
            if !child.is_null() {
                f(i, child);
            }
        }
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_CLOSURE {
        // Closure env (RFC 20260717 knife 3) — capture slots via the
        // knife-2 trace_fn (0 = no traceable captures), plus the +24
        // expando props dict every closure shares at a fixed offset.
        if let Some(trace) = unsafe { closure_trace_fn(p) } {
            let mut dyn_f: &mut dyn FnMut(u64, *mut c_void) = &mut f;
            unsafe {
                trace(
                    p,
                    trace_visit_tramp,
                    &mut dyn_f as *mut &mut dyn FnMut(u64, *mut c_void) as *mut c_void,
                )
            };
        }
        let props = unsafe { *((p as *const u8).add(CLOSURE_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else if matches!(
        unsafe { (*(p as *const HeapHeader)).type_tag },
        TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER | TAG_SYMBOL_WRAPPER
    ) {
        // Wrapper — single walkable child: the +16 expando props
        // dict (blade 3). The StringWrapper inner Str cell and the
        // SymbolWrapper inner Symbol cell are leaves, handled by the
        // teardown, never walked.
        let props = unsafe { *((p as *const u8).add(WRAPPER_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == crate::proxy::TAG_PROXY {
        // §10.5 — [[ProxyTarget]] and [[ProxyHandler]], both owned,
        // both ordinary objects. A handler that refers back to the
        // proxy it serves is a two-cell ring.
        for i in 0..crate::proxy::PROXY_CHILD_COUNT {
            let child = unsafe { crate::proxy::proxy_child_at(p, i) };
            if !child.is_null() {
                f(i, child);
            }
        }
    } else if let Some(off) =
        crate::layout_bag::bag_only_props_off(unsafe { (*(p as *const HeapHeader)).type_tag })
    {
        // Date / RegExp / Promise / the buffer family / the three
        // iterator cells — the lazy expando dict at the shape's own
        // offset. What is left after the arms below (compiled
        // program, buffer bytes, a promise's callback list) is the
        // destructor's, which `defer::finalize_all` runs on the
        // corpse.
        let props = unsafe { *((p as *const u8).add(off) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
        // Map and Set own two more references per entry. Key and
        // value are separate edges even when they are the same cell.
        let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
        if crate::iter_src::is_iter_helper(tag) {
            // §27.1.4.x helper — four owned AnyValue slots. A helper
            // whose callback closes over the helper is a ring, and so
            // is `it.flatMap(f)` where `f` yields something reaching
            // back; the immediate filter drops `take`/`drop`'s count.
            for i in 0..crate::iter_src::HELPER_CHILD_COUNT {
                let child = unsafe { crate::iter_src::helper_child_at(p, i) };
                if !child.is_null() {
                    f(i, child);
                }
            }
        }
        if crate::iter_src::is_iter_cell(tag) {
            // A stateful iterator holds a strong reference to what it
            // walks, so that iteration outlives the caller's binding.
            // `const it = a.values(); a.push(it)` is a two-cell ring.
            let src = unsafe { crate::iter_src::iter_src(p) };
            if !src.is_null() {
                f(crate::iter_src::ITER_SRC_SLOT, src);
            }
        }
        if matches!(tag, TAG_MAP | TAG_SET) {
            let n = unsafe { crate::map::map_child_count(p) };
            for i in 0..n {
                let child = unsafe { crate::map::map_child_at(p, i) };
                if !child.is_null() {
                    f(i, child);
                }
            }
        }
    } else {
        // TAG_ARR — element slots only when the kind guarantees
        // walkable bits (a scalar-kind array reaches here for its
        // expando alone), plus the +24 expando props dict (blade 3).
        if arr_elems_walkable(unsafe { &*(p as *const HeapHeader) }) {
            let n = unsafe { arr_len_of(p) };
            for i in 0..n {
                let child = unsafe { arr_child_at(p, i) };
                if !child.is_null() {
                    f(i, child);
                }
            }
        }
        let props = unsafe { *((p as *const u8).add(ARR_PROPS_OFF) as *const *mut c_void) };
        if !props.is_null() {
            f(PROPS_SLOT_INDEX, props);
        }
    }
}

/// Zero child slot `i` of `p` — shape-dispatched, index semantics per
/// [`for_each_child`]. Used by `collect_white`'s first sweep to break
/// the cycle before recursing.
///
/// # Safety
/// Same as [`for_each_child`]; `i` must be an index `for_each_child`
/// yielded for this `p`.
pub(crate) unsafe fn clear_child_slot(p: *mut c_void, i: u64) {
    if unsafe { is_class_obj(p) } {
        // The props sentinel MUST be tested before indexing
        // child_offsets — `child_offsets.add(u64::MAX)` is an OOB
        // read followed by a wild write (the closure arm's ordering).
        if i == PROPS_SLOT_INDEX {
            unsafe {
                *((p as *mut u8).add(OBJ_PROPS_OFF) as *mut *mut c_void) = core::ptr::null_mut()
            };
            return;
        }
        let lay = unsafe { layout_for_class_obj(p) };
        let off = unsafe { *(*lay).child_offsets.add(i as usize) };
        unsafe { *((p as *mut u8).add(off as usize) as *mut *mut c_void) = core::ptr::null_mut() };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_DYNOBJ {
        unsafe { dynobj_value_slot_clear(p, i) };
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == TAG_CLOSURE {
        if i == PROPS_SLOT_INDEX {
            unsafe {
                *((p as *mut u8).add(CLOSURE_PROPS_OFF) as *mut *mut c_void) = core::ptr::null_mut()
            };
        } else if let Some(trace) = unsafe { closure_trace_fn(p) } {
            // Only the trace body knows which address slot `i` maps
            // to (env slot vs capture-box interior) — replay it with
            // the clear trampoline. O(caps) on the cold collect path.
            let target: u64 = i;
            unsafe { trace(p, trace_clear_tramp, &target as *const u64 as *mut c_void) };
        }
    } else if unsafe { (*(p as *const HeapHeader)).type_tag } == crate::proxy::TAG_PROXY {
        unsafe { crate::proxy::proxy_slot_clear(p, i) };
    } else if i != PROPS_SLOT_INDEX
        && matches!(
            unsafe { (*(p as *const HeapHeader)).type_tag },
            TAG_MAP | TAG_SET
        )
    {
        // A Map / Set entry slot — the bag sentinel is handled by the
        // arm below, which is why this one tests for it first.
        unsafe { crate::map::map_slot_clear(p, i) };
    } else if i == crate::iter_src::ITER_SRC_SLOT
        && crate::iter_src::is_iter_cell(unsafe { (*(p as *const HeapHeader)).type_tag })
    {
        unsafe { crate::iter_src::iter_src_clear(p) };
    } else if i != PROPS_SLOT_INDEX
        && crate::iter_src::is_iter_helper(unsafe { (*(p as *const HeapHeader)).type_tag })
    {
        unsafe { crate::iter_src::helper_slot_clear(p, i) };
    } else if i == PROPS_SLOT_INDEX {
        // Wrapper +16 / bag-shape own offset / Arr +24 expando slot.
        let tag = unsafe { (*(p as *const HeapHeader)).type_tag };
        let off = if matches!(
            tag,
            TAG_NUMBER_WRAPPER | TAG_STRING_WRAPPER | TAG_BOOLEAN_WRAPPER | TAG_SYMBOL_WRAPPER
        ) {
            WRAPPER_PROPS_OFF
        } else {
            crate::layout_bag::bag_only_props_off(tag).unwrap_or(ARR_PROPS_OFF)
        };
        unsafe { *((p as *mut u8).add(off) as *mut *mut c_void) = core::ptr::null_mut() };
    } else {
        unsafe { arr_slot_clear(p, i) };
    }
}
