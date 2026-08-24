//! Per-family arm bodies behind the dispatch arm-seam族 (RFC
//! 20260824-s2-5 Phase B blade 2a).
//!
//! The dispatch skeleton ([`crate::method_call_cell::cell_method`]
//! and the immediate arms in [`crate::method_call`]) reaches every
//! per-tag family kernel through `extern "C"` declarations
//! ([`crate::dispatch_seam`]); `torajs-dispatch` owns the default
//! definitions as a separate archive member, forwarding here. A
//! compiler-emitted loud-reject stub in the user `.o` shadows a
//! family's default arm (user definitions win the vaddr table and
//! the member closure), the forward loses its in-edge, and the
//! family kernel plus its tables dead-strip.
//!
//! Every arm shares one C-ABI signature — the receiver's heap tag is
//! re-read here (one L1 load) instead of widening the seam surface,
//! so a specialized dispatcher only ever has to know ONE stub shape.
//! A family whose kernel answers `Option` floats its miss as
//! [`crate::method_call::ANY_METHOD_NO_SUCH`], exactly what the
//! skeleton's ladder returned before the seam existed.

use core::ffi::c_void;
use torajs_rc::Tag;

use crate::method_call::ANY_METHOD_NO_SUCH;
use crate::nanbox::{AnyValue, as_void_ptr};

#[inline]
unsafe fn tag_of(ptr: *mut c_void) -> u16 {
    unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() }
}

/// Str-cell arm (also the materialized short-str path).
///
/// # Safety
/// `recv` boxes a live Str cell; `argv` holds `argc` live slots.
pub unsafe fn str_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_str::str_method(as_void_ptr(recv) as *mut u8, mid, argv, argc) }
}

/// Arr-cell arm (the builtin surface after the skeleton's expando /
/// subclass / valueOf probes).
///
/// # Safety
/// `recv` boxes a live Arr cell; `recv_slot` is NULL or the
/// receiver variable's live slot; `argv` holds `argc` live slots.
pub unsafe fn arr_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_arr::arr_method(as_void_ptr(recv), mid, recv_slot, argv, argc) }
}

/// DynObj arm.
///
/// # Safety
/// `recv` boxes a live DynObj cell; other params as [`arr_arm_impl`].
pub unsafe fn dynobj_arm_impl(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        crate::method_call_dynobj::dynobj_method(
            as_void_ptr(recv),
            mid,
            name_str,
            recv_slot,
            argv,
            argc,
        )
    }
}

/// Static-layout struct (Tag::Obj) arm.
///
/// # Safety
/// `recv` boxes a live struct cell; other params as [`arr_arm_impl`].
pub unsafe fn struct_arm_impl(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        crate::method_call_dynobj::struct_method(as_void_ptr(recv), mid, name_str, argv, argc)
    }
}

/// Map / Set arm — the tag picks the flavor.
///
/// # Safety
/// `recv` boxes a live Map or Set cell.
pub unsafe fn mapset_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let ptr = as_void_ptr(recv);
    let is_set = unsafe { tag_of(ptr) } == Tag::Set as u16;
    unsafe { crate::method_call_mapset::map_set_method(ptr, is_set, mid, argv, argc) }
}

/// Iterator family arm — MapIter / ArrIter (lazy-helper chaining
/// first, RFC 20260730-iterator-global 刀 2) and IterHelper.
///
/// # Safety
/// `recv` boxes a live MapIter / ArrIter / IterHelper cell.
pub unsafe fn iter_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let ptr = as_void_ptr(recv);
    let tag = unsafe { tag_of(ptr) };
    unsafe {
        if tag == Tag::MapIter as u16 {
            if let Some(v) = crate::iter_helper::try_helper_chain(ptr, mid, argv, argc) {
                return v;
            }
            crate::method_call_mapset::map_iter_method(ptr, mid)
        } else if tag == Tag::ArrIter as u16 {
            if let Some(v) = crate::iter_helper::try_helper_chain(ptr, mid, argv, argc) {
                return v;
            }
            crate::method_call_mapset::arr_iter_method(ptr, mid)
        } else {
            crate::iter_helper::iter_helper_method(ptr, mid, argv, argc)
        }
    }
}

/// Buffer family arm — ArrayBuffer / TypedArray (species route
/// first, §23.2.4.1) / DataView. Kernel misses float
/// [`ANY_METHOD_NO_SUCH`], same as the pre-seam ladder.
///
/// # Safety
/// `recv` boxes a live ArrayBuffer / TypedArray / DataView cell.
pub unsafe fn buffer_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let tag = unsafe { tag_of(as_void_ptr(recv)) };
    unsafe {
        if tag == Tag::ArrayBuffer as u16 {
            crate::method_call_buffer::arraybuffer_method(recv, mid, argv, argc)
                .unwrap_or(ANY_METHOD_NO_SUCH)
        } else if tag == Tag::TypedArray as u16 {
            if let Some(v) =
                crate::method_call_buffer_species::ta_species_route(recv, mid, argv, argc)
            {
                return v;
            }
            crate::method_call_buffer::typedarray_method(recv, mid, argv, argc)
                .unwrap_or(ANY_METHOD_NO_SUCH)
        } else {
            crate::method_call_buffer_dataview::dataview_method(recv, mid, argv, argc)
                .unwrap_or(ANY_METHOD_NO_SUCH)
        }
    }
}

/// Date arm.
///
/// # Safety
/// `recv` boxes a live Date cell.
pub unsafe fn date_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_date::date_method(as_void_ptr(recv), mid, argv, argc) }
}

/// Promise arm — kernel misses float [`ANY_METHOD_NO_SUCH`].
///
/// # Safety
/// `recv` boxes a live Promise cell.
pub unsafe fn promise_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        crate::method_call_promise::promise_method(as_void_ptr(recv), mid, argv, argc)
            .unwrap_or(ANY_METHOD_NO_SUCH)
    }
}

/// RegExp arm.
///
/// # Safety
/// `recv` boxes a live RegExp cell.
pub unsafe fn regexp_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_regexp::regexp_method(as_void_ptr(recv), mid, argv, argc) }
}

/// BigInt arm.
///
/// # Safety
/// `recv` boxes a live BigInt cell.
pub unsafe fn bigint_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_bigint::bigint_method(as_void_ptr(recv), mid, argv, argc) }
}

/// Symbol-cell arm (§20.4.3.3 toString / inherited toLocaleString;
/// valueOf answered by the skeleton's cell-wide identity).
///
/// # Safety
/// `recv` boxes a live Symbol cell.
pub unsafe fn symbol_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    _argv: *const u64,
    _argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_cell::symbol_string_method(as_void_ptr(recv), mid) }
}

/// Closure arm (Function.prototype.call / apply, expando shadowing).
///
/// # Safety
/// `recv` boxes a live Closure cell.
pub unsafe fn closure_arm_impl(
    recv: AnyValue,
    mid: i64,
    name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe {
        crate::method_call_closure::closure_method(as_void_ptr(recv), mid, name_str, argv, argc)
    }
}

/// Weak family arm — WeakMap / WeakSet / WeakRef by tag.
///
/// # Safety
/// `recv` boxes a live WeakMap / WeakSet / WeakRef cell.
pub unsafe fn weak_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    let ptr = as_void_ptr(recv);
    let tag = unsafe { tag_of(ptr) };
    unsafe {
        if tag == Tag::WeakRef as u16 {
            crate::method_call_weak::weakref_method(ptr, mid)
        } else {
            crate::method_call_weak::weak_method(ptr, tag == Tag::WeakSet as u16, mid, argv, argc)
        }
    }
}

/// Number-immediate arm (int32 / double receivers).
///
/// # Safety
/// `recv` is an int32/double immediate; `argv` holds `argc` live
/// slots.
pub unsafe fn num_arm_impl(
    recv: AnyValue,
    mid: i64,
    _name_str: *const u8,
    _recv_slot: *mut u64,
    argv: *const u64,
    argc: i64,
) -> AnyValue {
    unsafe { crate::method_call_num::number_method(recv, mid, argv, argc) }
}
