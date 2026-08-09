//! `__torajs_arr_index_get` — kind-aware indexed element read for an
//! array reached through an `any` receiver.
//!
//! Any-dynamic-access RFC (20260704) S3. The dispatch entry
//! (`torajs-anyvalue::index_any::__torajs_any_index_get`) unboxes the
//! receiver and routes `Tag::Arr` cells here; this fn owns the array
//! layout knowledge:
//!
//! - `FLAG_ARR_ANY` — slots are NaN-box AnyValues; read directly.
//! - typed kinds (written by [`crate::mark_kind`] at the boxing
//!   boundary) — re-box the raw slot per `ARR_KIND_*`.
//! - `ARR_KIND_UNSET` — the array never crossed a marking boundary
//!   (a missed boxing site, or a > 21-level nesting chain tail);
//!   debug builds assert, release returns `undefined`.
//!
//! Out-of-bounds / negative index → `undefined` (ES §10.4.2.1), same
//! contract as `__torajs_arr_get_any_*`. Returned cell values carry
//! a +1 refcount (the slot keeps its own reference) — the SSA layer
//! releases the returned Any via the usual `anyv_rc_dec` drop.

use core::ffi::c_void;

use torajs_rc::{
    ARR_KIND_BOOL, ARR_KIND_F64, ARR_KIND_HEAP, ARR_KIND_I64, ARR_KIND_UNSET, FLAG_ARR_ANY,
    HeapHeader,
};

use crate::layout::{ARR_LEN_OFF, arr_data};

const ARR_HEAD_OFF: usize = 20;

unsafe extern "C" {
    /// Cross-tier — torajs-anyvalue NaN-box pack. Tag scheme:
    /// 0=Null, 1=Bool, 2=I64, 3=F64 (bits), 4=Heap, 5=Undef.
    fn __torajs_anyv_box_from_pair(tag: i64, value: i64) -> u64;
    /// Cross-tier — torajs-anyvalue NaN-box unpack (same tag scheme;
    /// ShortStr reports Heap and `unbox_value` materializes).
    fn __torajs_anyv_unbox_tag(v: u64) -> i64;
    fn __torajs_anyv_unbox_value(v: u64) -> i64;
    /// Cross-tier — torajs-rc. NaN-box-safe refcount bump (no-ops
    /// for non-cell bit patterns and NULL).
    fn __torajs_rc_inc(p: *mut c_void);
    /// Cross-tier — universal NaN-box-safe heap dropper.
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// Cross-tier — torajs-throw catchable errors (record + return).
    /// Signatures mirror the crate's prior declarations (`any.rs` /
    /// `throw_empty.rs`).
    fn __torajs_throw_range_error(msg: *const u8);
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// Cross-tier — the builtin-prototype singleton registry
    /// (torajs-rc/-core; link-time-resolved, `method_any_search`'s
    /// established pattern). Tag 2 = Array.prototype.
    fn __torajs_get_builtin_prototype(tag: i64) -> *mut c_void;
    /// Cross-tier — torajs-anyvalue %Object.prototype% digit-key
    /// probe, the chain tail of [`__torajs_arr_proto_index_get`].
    fn __torajs_object_proto_index_get(recv: *const c_void, idx: i64) -> u64;
}

/// NaN-box `undefined` sentinel via the pair packer (tag 5).
#[inline]
unsafe fn undef() -> u64 {
    unsafe { __torajs_anyv_box_from_pair(5, 0) }
}

/// Prototype digit-key probe for an element read that found no own
/// property (RFC 20260721 刀 5 G3) — §10.1.8.1 OrdinaryGet step 3.
///
/// The Array.prototype leg reads the tag-2 singleton's OWN element
/// face (`Array.prototype[1] = v` GROWS its element storage — see
/// `method_support_proto`'s length note): an accessor index runs its
/// getter against the ORIGINAL receiver, a hole falls through, and a
/// plain in-bounds slot answers through the kind-aware read (which
/// cannot re-enter this probe — its probe exits only fire on OOB /
/// hole). The %Object.prototype% tail lives in torajs-anyvalue.
/// `recv == singleton` (reading Array.prototype's own OOB index)
/// skips straight to the tail — no self-probe.
///
/// # Safety
/// `recv` is a live `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_proto_index_get(recv: *const c_void, idx: i64) -> u64 {
    unsafe {
        let ap = __torajs_get_builtin_prototype(2);
        if !ap.is_null() && !core::ptr::eq(ap as *const c_void, recv) && idx >= 0 {
            let len = *((ap as *const u8).add(ARR_LEN_OFF) as *const u64);
            if (idx as u64) < len {
                let hdr = &*(ap as *const HeapHeader);
                if hdr.flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
                    let pair = crate::define_accessor::__torajs_arr_index_accessor(
                        ap as *const c_void,
                        idx as u64,
                    );
                    if !pair.is_null() {
                        return crate::define_accessor::read_via_getter(pair, recv);
                    }
                    if crate::define::__torajs_arr_index_flags(ap as *const c_void, idx as u64)
                        & crate::define::F_HOLE
                        != 0
                    {
                        return __torajs_object_proto_index_get(recv, idx);
                    }
                }
                return __torajs_arr_index_get(ap as *const c_void, idx);
            }
        }
        __torajs_object_proto_index_get(recv, idx)
    }
}

/// Kind-aware `arr[idx]` read. See module doc for the contract.
///
/// # Safety
/// `arr` is either NULL or a valid `Tag::Arr` heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_get(arr: *const c_void, idx: i64) -> u64 {
    unsafe {
        if arr.is_null() {
            return undef();
        }
        let p = arr as *const u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if idx < 0 || idx as u64 >= len {
            // §10.1.8.1 step 3 — no own element: [[Get]] continues to
            // the Array.prototype / %Object.prototype% digit keys
            // (RFC 20260721 刀 5 G3).
            return __torajs_arr_proto_index_get(arr, idx);
        }
        let header = &*(arr as *const HeapHeader);
        // Sparse tail (RFC 20260810-arr-sparse-grow) — `[extent,
        // len)` is implicit holes with no storage: same prototype
        // continuation as an explicit hole, before anything touches
        // the slot buffer.
        if header.flags & torajs_rc::FLAG_ARR_SPARSE_TAIL != 0
            && idx as u64 >= crate::layout::arr_live_extent(p)
        {
            return __torajs_arr_proto_index_get(arr, idx);
        }
        // Exotic slow path (chunk C accessor) — an accessor index
        // reads through its getter; plain arrays never take this
        // branch (one predictable bit test).
        if header.flags & torajs_rc::FLAG_ARR_EXOTIC_INDEX != 0 {
            let pair = crate::define_accessor::__torajs_arr_index_accessor(arr, idx as u64);
            if !pair.is_null() {
                return crate::define_accessor::read_via_getter(pair, arr);
            }
            // A hole (elision / delete / length-grow) is not an own
            // property — same prototype continuation as OOB.
            if crate::define::__torajs_arr_index_flags(arr, idx as u64) & crate::define::F_HOLE != 0
            {
                return __torajs_arr_proto_index_get(arr, idx);
            }
        }
        let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
        let slot = arr_data(p).add(((head + idx as u64) as usize) * 8);
        let raw = *(slot as *const u64);
        if header.flags & FLAG_ARR_ANY != 0 {
            // NaN-box slot — the slot keeps its own reference, the
            // returned copy takes another (+1 for cells, no-op for
            // immediates).
            __torajs_rc_inc(raw as *mut c_void);
            return raw;
        }
        match header.arr_elem_kind() {
            ARR_KIND_I64 => __torajs_anyv_box_from_pair(2, raw as i64),
            ARR_KIND_F64 => __torajs_anyv_box_from_pair(3, raw as i64),
            ARR_KIND_BOOL => __torajs_anyv_box_from_pair(1, raw as i64),
            ARR_KIND_HEAP => {
                // A null slot is a hole (e.g. a missed optional
                // capture in a match array) — `undefined` per spec,
                // never a boxed null pointer.
                if raw == 0 {
                    return undef();
                }
                __torajs_rc_inc(raw as *mut c_void);
                __torajs_anyv_box_from_pair(4, raw as i64)
            }
            kind => {
                debug_assert!(
                    kind == ARR_KIND_UNSET,
                    "arr_index_get: invalid elem kind {kind}"
                );
                debug_assert!(
                    false,
                    "arr_index_get: UNSET elem kind — a typed-arr→Any \
                     boxing site missed __torajs_arr_mark_kind"
                );
                undef()
            }
        }
    }
}

/// Kind-aware `arr[idx] = (tag, value)` write for an array reached
/// through an `any` receiver (RFC 20260704 S3-set). Pair ABI mirrors
/// `__torajs_arr_set_any` / ssa-lower's `pack_any_slot_value` — for
/// `tag == 4` the caller transfers ownership of one rc.
///
/// - `FLAG_ARR_ANY` — delegate to [`crate::any`]'s
///   `__torajs_arr_set_any` (drop-old + box-store, native ledger).
/// - raw-scalar kinds — store the matching raw repr; `number`
///   semantics let an integral f64 land in an I64 slot and an int32
///   widen into an F64 slot. A value whose repr can't be stored
///   without changing the array's element kind raises a catchable
///   TypeError — element-kind transitions (V8 style) are the RFC's
///   S7+ follow-up, never a silent corruption.
/// - `ARR_KIND_HEAP` — rejected (TypeError): the 3-bit kind can't
///   verify the *static* element type, so a through-any heap-element
///   store could corrupt the typed tier.
/// - OOB / negative index — catchable RangeError (tr arrays don't
///   sparse-grow on assignment; same contract as `arr_set_any`).
///
/// Rejection paths release a transferred `tag == 4` rc before
/// raising, keeping the pair ledger balanced for catch-and-continue
/// programs.
///
/// # Safety
/// `arr` is either NULL or a valid `Tag::Arr` heap pointer; a
/// `tag == 4` `value` must be 0 or a valid owned heap pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_arr_index_set(arr: *mut c_void, idx: i64, tag: u64, value: u64) {
    unsafe {
        if arr.is_null() {
            drop_pair(tag, value);
            return;
        }
        let header = &*(arr as *const HeapHeader);
        if header.flags & FLAG_ARR_ANY != 0 {
            // Native Arr<Any> ledger (drop-old + box-store + OOB
            // RangeError) — includes negative-index rejection via
            // the u64 cast.
            crate::any::__torajs_arr_set_any(arr, idx as u64, tag, value);
            return;
        }
        let p = arr as *mut u8;
        let len = *(p.add(ARR_LEN_OFF) as *const u64);
        if idx < 0 || idx as u64 >= len {
            drop_pair(tag, value);
            __torajs_throw_range_error(
                c"out-of-bounds index write through an any receiver".as_ptr() as *const u8,
            );
            return;
        }
        let head = *(p.add(ARR_HEAD_OFF) as *const u32) as u64;
        let slot = arr_data(p).add(((head + idx as u64) as usize) * 8) as *mut u64;
        let kind = header.arr_elem_kind();
        // Scalar coercion table shared with the chunk 622 typed
        // writers (`any_typed_bridge`); every HEAP case is None.
        let Some(raw) = crate::any_typed_bridge::coerce_raw_scalar(kind, tag, value) else {
            return kind_mismatch(tag, value);
        };
        *slot = raw;
    }
}

/// Release a transferred `tag == 4` rc (no-op for immediates).
unsafe fn drop_pair(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}

/// Shared catchable-TypeError tail for element-kind-mismatch writes.
unsafe fn kind_mismatch(tag: u64, value: u64) {
    unsafe {
        drop_pair(tag, value);
        __torajs_throw_type_error(
            c"assignment through an any array receiver would change the array's element kind"
                .as_ptr(),
        );
    }
}
