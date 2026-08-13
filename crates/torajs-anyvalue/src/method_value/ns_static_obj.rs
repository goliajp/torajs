//! Object / Symbol namespace-static arm bodies — split from
//! `method_value/ns_static.rs` under the 500-line file rule (the
//! batch-6 rows tipped it). Holds the own-enumeration family, the
//! assign fold and the Symbol registry pair; the dispatch match and
//! the cell mint / reflection probes stay in the parent.

use crate::nanbox::VALUE_UNDEFINED;

use super::ns_static::{arg_at, own};
use super::ns_static_table::{
    __torajs_anyv_assign, __torajs_anyv_own_entries, __torajs_anyv_own_keys,
    __torajs_anyv_own_keys_all, __torajs_anyv_own_symbols, __torajs_anyv_own_values,
    __torajs_arr_mark_kind, __torajs_str_drop, __torajs_symbol_for, __torajs_symbol_key_for,
    __torajs_throw_check, __torajs_throw_type_error, OwnKind,
};

use core::ffi::c_void;
use torajs_rc::Tag;

/// `ARR_KIND_HEAP` mirror (torajs-rc) — the one kind an own-keys
/// block ever holds.
const KIND_HEAP_CHAIN: u64 = 4;

/// §20.1.2.{17,23,5} — the kernel answers a fresh Arr cell (rc 1),
/// which IS the owned result (no inc). A null/undefined receiver
/// records its ToObject TypeError inside the kernel and still answers
/// a well-formed empty Arr; the caller's throw check makes it
/// unobservable, exactly as on the typed tier
/// (`ssa_lower_call_object_keys.rs`).
pub(super) unsafe fn own_enum(kind: &OwnKind, recv: u64) -> u64 {
    unsafe {
        let arr = match kind {
            OwnKind::Keys | OwnKind::Names => {
                let a = __torajs_anyv_own_keys(
                    recv,
                    if matches!(kind, OwnKind::Names) { 1 } else { 0 },
                );
                // Slots are Str heap pointers, and this call IS the
                // typed→Any boundary: without the stamp the any-lane
                // drop frees the block and strands every key cell.
                // Literal-keyed objects hide it (static Str skips
                // its drop), runtime-added keys do not.
                __torajs_arr_mark_kind(a, KIND_HEAP_CHAIN);
                a
            }
            // Already self-describing: `own_values` answers a real
            // Array<Any>, `own_entries` stamps its outer block.
            // Stamping the values array HEAP would be actively
            // wrong — its slots hold immediates too, and a walker
            // would deref a small int.
            OwnKind::Values => __torajs_anyv_own_values(recv),
            OwnKind::Entries => __torajs_anyv_own_entries(recv),
        };
        arr as u64
    }
}

/// §20.1.2.10 — the own symbol keys as a fresh owned `Arr`; a nullish
/// receiver's ToObject TypeError is pending-recorded and the caller's
/// throw check surfaces it. Stamped like the own-keys arm: the slots
/// are heap pointers.
pub(super) unsafe fn own_symbols_value(recv: u64) -> u64 {
    unsafe {
        let a = __torajs_anyv_own_symbols(recv);
        __torajs_arr_mark_kind(a, KIND_HEAP_CHAIN);
        a as u64
    }
}

/// §28.1.11 — the string buckets followed by the symbol bucket, which
/// is what `Reflect.ownKeys` answers and what neither of the two
/// narrower faces gives on its own. Same stamp: every slot is a heap
/// pointer (Str cells then Symbol cells).
pub(super) unsafe fn own_keys_all_value(recv: u64) -> u64 {
    unsafe {
        let a = __torajs_anyv_own_keys_all(recv);
        __torajs_arr_mark_kind(a, KIND_HEAP_CHAIN);
        a as u64
    }
}

/// §20.1.2.1 — variadic fold over the sources; the target answers as
/// an owned reference. With no sources, one undefined-source call
/// still runs so the kernel's own step-1 ToObject guard fires
/// (single-sourced instead of re-derived here).
pub(super) unsafe fn object_assign(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let target = arg_at(argv, argc, 0);
        if argc < 2 {
            __torajs_anyv_assign(target, VALUE_UNDEFINED);
            if __torajs_throw_check() != 0 {
                return VALUE_UNDEFINED;
            }
        } else {
            for i in 1..argc {
                __torajs_anyv_assign(target, *argv.add(i as usize));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
            }
        }
        own(target)
    }
}

/// §20.4.2.2 step 1 ToString(key) — missing arg coerces undefined →
/// "undefined" (bun agrees). The kernel SHARES the key (a registry
/// miss incs the desc itself), so the minted temp stays ours to
/// release; the returned Symbol is owned.
pub(super) unsafe fn symbol_for_value(key_any: u64) -> u64 {
    unsafe {
        let key = crate::nanbox_ffi::__torajs_anyv_to_str(key_any);
        if __torajs_throw_check() != 0 {
            return VALUE_UNDEFINED;
        }
        let sym = __torajs_symbol_for(key);
        __torajs_str_drop(key);
        sym as u64
    }
}

/// §20.4.2.6 — step 1 rejects a non-Symbol loudly (no coercion); an
/// unregistered symbol answers undefined, never a raw null Str slot
/// (the §25.5.2 sentinel lesson).
pub(super) unsafe fn symbol_key_for_value(v: u64) -> u64 {
    unsafe {
        let is_sym = crate::nanbox::is_cell(v) && {
            let ptr = crate::nanbox::as_void_ptr(v);
            (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Symbol as u16
        };
        if !is_sym {
            __torajs_throw_type_error(c"Symbol.keyFor requires a symbol".as_ptr());
            return VALUE_UNDEFINED;
        }
        let key = __torajs_symbol_key_for(crate::nanbox::as_void_ptr(v));
        if key.is_null() {
            return VALUE_UNDEFINED;
        }
        key as u64
    }
}

/// Compiler face for the any-arg direct-call lane (RFC
/// 20260720-symbol-any-call-boundary) — same kernel the dispatcher
/// arm uses, so ToString coercion and its throw face never drift.
/// Arg is a borrow; the returned Symbol is owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_for_any(key_any: u64) -> u64 {
    unsafe { symbol_for_value(key_any) }
}

/// Compiler face for `Symbol.keyFor(x: any)` — §20.4.2.6 brand check
/// (non-Symbol throws), unregistered answers VALUE_UNDEFINED. Arg is
/// a borrow; a hit's key Str comes back owned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_symbol_key_for_any(v: u64) -> u64 {
    unsafe { symbol_key_for_value(v) }
}

unsafe extern "C" {
    /// torajs-arr — fresh `Array<Any>` mint + push (the same pair
    /// `ns_static.rs`'s iterator_concat_pack declares).
    fn __torajs_arr_alloc_any(cap: u64) -> *mut u8;
    fn __torajs_arr_push_any(arr: *mut c_void, tag: u64, value: u64) -> *mut u8;
    /// torajs-regex — ES2025 §22.2.5.1 EncodeForRegExpEscape over a
    /// live Str; answers a fresh Str.
    fn __torajs_regexp_escape(s: *const c_void) -> *mut u8;
}

/// §23.1.2.3 Array.of as a detached call — pack argv into a fresh
/// `Array<Any>` (the `iterator_concat_pack` shape without the kernel
/// hop). Each arg's payload rc-incs on entry; the minted array is
/// the owned answer.
pub(super) unsafe fn array_of_pack(argv: *const u64, argc: i64) -> u64 {
    unsafe {
        let n = argc.max(0);
        let mut items = __torajs_arr_alloc_any(n as u64);
        for i in 0..n {
            let v = arg_at(argv, argc, i);
            let t = crate::__torajs_anyv_unbox_tag(v);
            let p = crate::__torajs_anyv_unbox_value(v);
            crate::payload_rc_inc(t, p);
            items = __torajs_arr_push_any(items as *mut c_void, t as u64, p as u64);
        }
        crate::nanbox::box_void_ptr(items as *mut c_void)
    }
}

/// ES2025 §22.2.5.1 RegExp.escape — step 1 is a STRICT String check
/// (no ToString: every non-string throws). A ShortStr materializes
/// to a heap Str first (released after the kernel run); a Str cell
/// rides the torajs-regex escape kernel directly. The fresh escaped
/// Str comes back boxed.
pub(super) unsafe fn regexp_escape_value(v: u64) -> u64 {
    unsafe {
        if crate::nanbox::is_short_str(v) {
            let tmp = crate::nanbox_ffi_materialize::materialize_short_str(v);
            let out = __torajs_regexp_escape(tmp as *const c_void);
            crate::nanbox_ffi_materialize::drop_materialized_str(tmp);
            return crate::nanbox::box_void_ptr(out as *mut c_void);
        }
        if crate::nanbox::is_cell(v) {
            let p = v as *const c_void;
            let tag = (p.cast::<u8>().add(4) as *const u16).read();
            if tag == Tag::Str as u16 {
                return crate::nanbox::box_void_ptr(__torajs_regexp_escape(p) as *mut c_void);
            }
        }
        super::ns_static_table::__torajs_throw_type_error(
            c"RegExp.escape requires a string".as_ptr(),
        );
        VALUE_UNDEFINED
    }
}

/// Compiler face for the typed/any direct-call lane — same shell the
/// dispatcher arm uses, so the strict String gate never drifts.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_regexp_escape_any(v: u64) -> u64 {
    unsafe { regexp_escape_value(v) }
}
