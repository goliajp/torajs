//! `__torajs_any_index_set` — `recv[idx] = v` where the receiver is
//! an `any` value (write dual of [`crate::index_any`]'s read tree;
//! split out at 刀 9 when the wrapper arm pushed the shared file
//! over the 500-line limit). Verbatim move — the dispatch arms and
//! the pair-ABI ownership ledger are unchanged.

use core::ffi::c_void;

use torajs_rc::{FLAG_ARR_EXOTIC_INDEX, FLAG_FROZEN, FLAG_NON_EXTENSIBLE, FLAG_SEALED, Tag};

use crate::index_any::i64_dec;

use crate::nanbox::{AnyValue, as_void_ptr, is_cell, is_null, is_undefined};

unsafe extern "C" {
    /// torajs-arr — kind-aware `arr[idx] = (tag, value)` (pair ABI,
    /// tag 4 transfers one rc).
    fn __torajs_arr_index_set(arr: *mut c_void, idx: i64, tag: u64, value: u64);
    /// torajs-arr — §10.4.2.1 store refusal for one index (writable
    /// / hole-vs-extensible, integrity level folded in). Shared with
    /// the typed tier's emitted pre-store gate.
    fn __torajs_arr_index_refuses_store(arr: *const c_void, idx: u64) -> i32;
    /// Universal NaN-box-safe heap dropper (torajs-value-drop).
    fn __torajs_value_drop_heap(p: *mut c_void);
    /// torajs-str — release a heap Str reference.
    fn __torajs_str_drop(s: *mut c_void);
    /// torajs-throw — record a pending catchable TypeError.
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-str — heap Str constructor (rc=1) for the decimal key.
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    /// torajs-dynobj — keyed store (resize writes the relocated
    /// block back through the slot).
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
}

/// `recv[idx] = (tag, value)` where the receiver is an `any` value
/// (RFC 20260704 S3-set). Pair ABI mirrors ssa-lower's
/// `pack_any_slot_value` — for `tag == 4` the caller transfers
/// ownership of one rc; every path that doesn't store the pair
/// releases it.
///
/// - `null` / `undefined` receiver → catchable TypeError.
/// - primitive receivers (numbers / bools / strings — including
///   heap Str/Substr cells) → silent no-op, matching non-strict JS
///   assignment-to-primitive-property semantics (§13.15.2 PutValue on
///   a primitive base discards in sloppy mode; strings are immutable
///   either way).
/// - `Tag::Arr` cell → `__torajs_arr_index_set` (kind-aware; OOB →
///   catchable RangeError, kind mismatch → catchable TypeError).
/// - `Tag::DynObj` (L3b #3, chunk 527) → the numeric key
///   stringifies to its decimal form and stores through
///   `__torajs_dynobj_set` (the pair transfers into the bucket); a
///   resize-relocated block writes the fresh cell back through
///   `recv_slot` (NULL for non-Ident receivers — same canonical-slot
///   shape as the member-set gate).
/// - any other heap tag → explicit TypeError.
///
/// # Safety
/// Cell receivers must be valid heap pointers matching their header
/// tag layout; a `tag == 4` `value` must be 0 or a valid owned heap
/// pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_any_index_set(
    recv: AnyValue,
    idx: i64,
    tag: u64,
    value: u64,
    recv_slot: *mut u64,
) {
    if is_null(recv) || is_undefined(recv) {
        unsafe {
            drop_transferred_pair(tag, value);
            __torajs_throw_type_error(c"cannot set properties of null or undefined".as_ptr());
        }
        return;
    }
    if !is_cell(recv) {
        unsafe { drop_transferred_pair(tag, value) };
        return;
    }
    let ptr = as_void_ptr(recv);
    let hdr_tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    if hdr_tag == Tag::Arr as u16 {
        // §10.4.2.1 / OrdinarySetWithOwnDescriptor — a non-writable
        // own index refuses the store, and a module-strict program
        // turns that refusal into a TypeError. The element domain has
        // no per-slot attribute storage, so the flags probe folds in
        // both the defineProperty shadow and the cell's integrity
        // level; an out-of-range index owns nothing and falls through
        // to the kernel's own RangeError. Pre-fix `Object.freeze(a)`
        // left `a[i] = v` MUTATING the frozen array (and a
        // `writable: false` index was equally writable), while
        // `Object.isFrozen` answered true.
        //
        // The probe is a cross-staticlib call, so the hot path skips
        // it on the header word already loaded for `hdr_tag`: an
        // index can only be non-writable through a defineProperty
        // shadow (the exotic bit) or an integrity level, and a plain
        // array carries neither.
        let hflags = unsafe { (ptr.cast::<u8>().add(6) as *const u16).read() };
        if idx >= 0
            && hflags & (FLAG_ARR_EXOTIC_INDEX | FLAG_FROZEN | FLAG_SEALED | FLAG_NON_EXTENSIBLE)
                != 0
            && unsafe { __torajs_arr_index_refuses_store(ptr, idx as u64) } != 0
        {
            unsafe {
                drop_transferred_pair(tag, value);
                __torajs_throw_type_error(c"cannot assign to a read-only property".as_ptr());
            }
            return;
        }
        unsafe { __torajs_arr_index_set(ptr, idx, tag, value) };
        return;
    }
    if hdr_tag == Tag::Str as u16 {
        unsafe { drop_transferred_pair(tag, value) };
        return;
    }
    if hdr_tag == Tag::DynObj as u16 {
        let mut buf = [0u8; 20];
        let (start, len) = i64_dec(&mut buf, idx);
        unsafe {
            let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
            let mut obj = ptr;
            __torajs_dynobj_set(&mut obj, key as *mut c_void, tag, value);
            __torajs_str_drop(key as *mut c_void);
            if obj != ptr && !recv_slot.is_null() {
                // Resize relocated the block — the NaN-box cell
                // encoding is the pointer bits; transfer, no rc
                // traffic (same identity, moved storage).
                *recv_slot = crate::nanbox_encode::__torajs_anyv_box_from_pair(4, obj as i64);
            }
        }
        return;
    }
    // RFC 20260721 刀 9 G2c — primitive-wrapper indexed write: the
    // decimal key stores through the member-set wrapper arm (lazy
    // `+16` expando dynobj, StringWrapper non-writable index domain,
    // non-extensible gate all live there). The wrapper cell's
    // identity never moves — only the props slot's stored pointer —
    // so the local recv copy needs no writeback.
    // Blade 2 (RFC 20260714-struct-dynamic-props) — a struct
    // receiver's numeric write rides the same member-set route as
    // the wrappers below: the decimal key hits a layout field named
    // by its §7.1.19 spelling or lands in the +24 expando dict (the
    // non-extensible gate lives in the struct arm). The struct
    // cell's identity never moves either.
    if hdr_tag == Tag::NumberWrapper as u16
        || hdr_tag == Tag::StringWrapper as u16
        || hdr_tag == Tag::BooleanWrapper as u16
        || hdr_tag == Tag::Obj as u16
    {
        let mut buf = [0u8; 20];
        let (start, len) = i64_dec(&mut buf, idx);
        unsafe {
            let key = __torajs_str_alloc(buf[start..].as_ptr(), len as i64);
            let mut recv_local = recv;
            crate::member_set::__torajs_any_member_set(
                &mut recv_local,
                key as *mut c_void,
                tag,
                value,
                0,
            );
            __torajs_str_drop(key as *mut c_void);
        }
        return;
    }
    unsafe {
        drop_transferred_pair(tag, value);
        __torajs_throw_type_error(
            c"indexed write on this receiver through any is not yet implemented".as_ptr(),
        );
    }
}

/// Release a transferred `tag == 4` rc (no-op for immediates).
unsafe fn drop_transferred_pair(tag: u64, value: u64) {
    if tag == 4 {
        unsafe { __torajs_value_drop_heap(value as *mut c_void) };
    }
}
