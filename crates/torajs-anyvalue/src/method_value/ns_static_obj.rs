//! Object / Symbol namespace-static arm bodies — split from
//! `method_value/ns_static.rs` under the 500-line file rule (the
//! batch-6 rows tipped it). Holds the own-enumeration family, the
//! assign fold and the Symbol registry pair; the dispatch match and
//! the cell mint / reflection probes stay in the parent.

use crate::nanbox::VALUE_UNDEFINED;

use super::ns_static::{arg_at, own};
use super::ns_static_table::{
    __torajs_anyv_assign, __torajs_anyv_own_entries, __torajs_anyv_own_keys,
    __torajs_anyv_own_symbols, __torajs_anyv_own_values, __torajs_arr_mark_kind, __torajs_str_drop,
    __torajs_symbol_for, __torajs_symbol_key_for, __torajs_throw_check, __torajs_throw_type_error,
    OwnKind,
};

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

/// §20.1.2.10 — the kernel answers a fresh (owned) empty `Arr<Str>`
/// (tr has no symbol-keyed props, the W-N-c truth); a nullish
/// receiver's ToObject TypeError is pending-recorded, the caller's
/// throw check surfaces it. Stamped like the own-keys arm: slots
/// (if the symbol surface ever lands) are heap pointers.
pub(super) unsafe fn own_symbols_value(recv: u64) -> u64 {
    unsafe {
        let a = __torajs_anyv_own_symbols(recv);
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
