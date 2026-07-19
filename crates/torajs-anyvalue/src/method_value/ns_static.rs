//! Namespace-static value family (RFC 20260719-ns-static-value-reify
//! B1) — `Math.max` read as a VALUE answers an interned immortal
//! closure cell whose boxed dual entry is a REAL dispatcher:
//! namespace statics are receiver-less, so a bare call (`const m =
//! Math.max; m(1, 5)`), a `.call/.apply` re-dispatch (thisArg
//! ignored per §21.3), an any-lane call and a HOF boxed callback all
//! work through the same entry. The compiler routes alias calls
//! through the variadic boxed lane (`variadic_locals`), so the cell
//! never needs a per-signature typed entry; `fn_addr` stays a loud
//! TypeError for the typed-slot direct-call boundary (RFC B4).
//!
//! Ids come from the shared `torajs_rc::ns_static` table (the same
//! truth the compiler interns at lower time); the dispatch table
//! below is index-lockstep with it (unit-tested). Math semantics
//! delegate to the SAME `__torajs_math_*` kernels the typed tier
//! calls — single source, zero drift.
//!
//! The `Disp` shape enum, the id-indexed `DISPATCH` table and the
//! kernel externs live in the `ns_static_table` sibling (file-size
//! rule); this module holds the cell mint, the reflection probes and
//! the per-shape dispatch arms.

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::ns_static::ns_static_meta;
use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::{VALUE_UNDEFINED, box_double, box_int32};

use super::ns_static_table::{
    __torajs_anyv_assign, __torajs_anyv_freeze, __torajs_anyv_from_entries,
    __torajs_anyv_get_proto_of_any, __torajs_anyv_own_entries, __torajs_anyv_own_keys,
    __torajs_anyv_own_values, __torajs_anyv_set_prototype_of, __torajs_arr_mark_kind,
    __torajs_math_max, __torajs_math_min, __torajs_num_parse_float, __torajs_num_parse_int,
    __torajs_obj_is_frozen_any, __torajs_str_drop, __torajs_throw_check, __torajs_throw_type_error,
    DISPATCH, Disp, NumPred, OwnKind,
};

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, TABLE_SIZE, mint_immortal_str,
};

/// `ARR_KIND_HEAP` mirror (torajs-rc) — the one kind an own-keys
/// block ever holds.
const KIND_HEAP_CHAIN: u64 = 4;

/// Separator/newline byte between the tag-aware per-arg prints —
/// the Rust-path call (torajs-io is a real Cargo dep) keeps the
/// symbol linked in test binaries, where an extern-only reference
/// leaves the rlib member dead-stripped.
fn putc_stdout(c: u8) {
    torajs_io::__torajs_io_putc_stdout(i32::from(c));
}

/// Per-id interned cells + `.name` Str cells — same immortal
/// atomic-static shape as `METHOD_CELLS` (headroom bound shared with
/// the mid table; lockstep-tested below).
static NS_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];
static NS_NAME_CELLS: [AtomicU64; TABLE_SIZE] = [const { AtomicU64::new(0) }; TABLE_SIZE];

/// Boxed dual entry of every ns-static cell — receiver-less real
/// dispatch (`this` is ignored: `invoke_with_this` never shifts a
/// non-recv-first cell, and a bare call passes no receiver at all).
unsafe extern "C" fn ns_dispatch_entry(env: *mut c_void, argv: *const u64, argc: i64) -> u64 {
    let id = unsafe { *(env.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64 };
    unsafe { dispatch(id, argv, argc) }
}

/// `fn_addr` of every ns-static cell — an any→typed fn-slot cast
/// that direct-calls the native entry throws instead of jumping into
/// a wrong-ABI body (RFC B4 records the typed-slot adapter face).
unsafe extern "C" fn ns_native_entry() -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"builtin namespace static called through a typed fn slot".as_ptr(),
        );
    }
    0
}

/// ToNumber(argv[i]) with the spec's abrupt-completion contract: a
/// pending throw recorded during coercion (poisoned valueOf) aborts
/// the caller's remaining coercions. Missing args coerce undefined.
unsafe fn arg_num(argv: *const u64, argc: i64, i: i64) -> Result<f64, ()> {
    let v = if i < argc {
        unsafe { *argv.add(i as usize) }
    } else {
        VALUE_UNDEFINED
    };
    let n = unsafe { crate::nanbox_ffi::__torajs_anyv_to_number(v) };
    if unsafe { __torajs_throw_check() } != 0 {
        return Err(());
    }
    Ok(n)
}

/// ES §7.1.6 ToInt32-shaped f64 → i64 with modular (non-saturating)
/// wrap — the math kernels truncate to 32 bits themselves, so the
/// only job here is keeping huge doubles out of Rust's saturating
/// `as i64` cast.
fn to_i64_mod32(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc() % 4294967296.0) as i64
}

unsafe fn dispatch(id: i64, argv: *const u64, argc: i64) -> u64 {
    let Some(disp) = (if id < 0 {
        None
    } else {
        DISPATCH.get(id as usize)
    }) else {
        // Unreachable through minted cells (the compiler bakes only
        // table hits); loud instead of garbage if it ever is.
        unsafe {
            __torajs_throw_type_error(c"unknown namespace-static id".as_ptr());
        }
        return VALUE_UNDEFINED;
    };
    unsafe {
        match disp {
            Disp::F(f) => match arg_num(argv, argc, 0) {
                Ok(x) => box_double(f(x)),
                Err(()) => VALUE_UNDEFINED,
            },
            Disp::Ff(f) => {
                let Ok(x) = arg_num(argv, argc, 0) else {
                    return VALUE_UNDEFINED;
                };
                let Ok(y) = arg_num(argv, argc, 1) else {
                    return VALUE_UNDEFINED;
                };
                box_double(f(x, y))
            }
            Disp::MinMax { is_max } => {
                // §21.3.2.24/25 — coerce every arg in source order,
                // fold pairwise through the typed-tier kernel (NaN
                // propagation and ±0 ordering live there).
                let mut acc = if *is_max {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                for i in 0..argc {
                    let Ok(x) = arg_num(argv, argc, i) else {
                        return VALUE_UNDEFINED;
                    };
                    acc = if *is_max {
                        __torajs_math_max(acc, x)
                    } else {
                        __torajs_math_min(acc, x)
                    };
                }
                box_double(acc)
            }
            Disp::I32Pair(f) => {
                let Ok(x) = arg_num(argv, argc, 0) else {
                    return VALUE_UNDEFINED;
                };
                let Ok(y) = arg_num(argv, argc, 1) else {
                    return VALUE_UNDEFINED;
                };
                box_int32(f(to_i64_mod32(x), to_i64_mod32(y)) as i32)
            }
            Disp::I32One(f) => match arg_num(argv, argc, 0) {
                Ok(x) => box_int32(f(to_i64_mod32(x)) as i32),
                Err(()) => VALUE_UNDEFINED,
            },
            Disp::Nullary(f) => box_double(f()),
            Disp::ConsoleLog => {
                for i in 0..argc {
                    if i > 0 {
                        putc_stdout(b' ');
                    }
                    crate::inspect::any::__torajs_print_anyv_inline_top(*argv.add(i as usize));
                }
                putc_stdout(b'\n');
                VALUE_UNDEFINED
            }
            Disp::ParseInt => {
                let v = arg_at(argv, argc, 0);
                let s = crate::nanbox_ffi::__torajs_anyv_to_str(v);
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let Ok(radix) = arg_num(argv, argc, 1) else {
                    __torajs_str_drop(s);
                    return VALUE_UNDEFINED;
                };
                let n = __torajs_num_parse_int(s as *const u8, to_i64_mod32(radix));
                __torajs_str_drop(s);
                box_double(n)
            }
            Disp::ParseFloat => {
                let s = crate::nanbox_ffi::__torajs_anyv_to_str(arg_at(argv, argc, 0));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                let n = __torajs_num_parse_float(s as *const u8);
                __torajs_str_drop(s);
                box_double(n)
            }
            Disp::NumPredicate(p) => box_bool(num_predicate(p, arg_at(argv, argc, 0))),
            Disp::ArrayIsArray => {
                let v = arg_at(argv, argc, 0);
                let hit = crate::nanbox::is_cell(v) && {
                    let ptr = crate::nanbox::as_void_ptr(v);
                    (ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Arr as u16
                };
                box_bool(hit)
            }
            Disp::ObjectIs => box_bool(crate::nanbox_ffi::__torajs_anyv_same_value(
                arg_at(argv, argc, 0),
                arg_at(argv, argc, 1),
            )),
            // The kernel answers a fresh Arr cell (rc 1) — that IS
            // the owned result, so no inc. A null/undefined receiver
            // records its ToObject TypeError inside the kernel and
            // still answers a well-formed empty Arr; the caller's
            // throw check makes it unobservable, exactly as on the
            // typed tier (`ssa_lower_call_object_keys.rs`).
            Disp::OwnEnum(kind) => {
                let recv = arg_at(argv, argc, 0);
                let arr = match kind {
                    OwnKind::Keys => {
                        let a = __torajs_anyv_own_keys(recv, 0);
                        // Slots are Str heap pointers, and this call
                        // IS the typed→Any boundary: without the
                        // stamp the any-lane drop frees the block
                        // and strands every key cell. Literal-keyed
                        // objects hide it (static Str skips its
                        // drop), runtime-added keys do not.
                        __torajs_arr_mark_kind(a, KIND_HEAP_CHAIN);
                        a
                    }
                    // Already self-describing: `own_values` answers a
                    // real Array<Any>, `own_entries` stamps its outer
                    // block. Stamping the values array HEAP would be
                    // actively wrong — its slots hold immediates too,
                    // and a walker would deref a small int.
                    OwnKind::Values => __torajs_anyv_own_values(recv),
                    OwnKind::Entries => __torajs_anyv_own_entries(recv),
                };
                arr as u64
            }
            Disp::ObjectAssign => {
                let target = arg_at(argv, argc, 0);
                if argc < 2 {
                    // §20.1.2.1 step 1 ToObject(target) runs even with
                    // no sources. Call once with an undefined source:
                    // the kernel guards the target itself and treats
                    // an undefined source as a no-op, so step 1 stays
                    // single-sourced instead of re-derived here.
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
            Disp::ObjectFreeze => own(__torajs_anyv_freeze(arg_at(argv, argc, 0))),
            Disp::ObjectIsFrozen => {
                box_bool(__torajs_obj_is_frozen_any(arg_at(argv, argc, 0) as i64))
            }
            // Already owned on return — inc'ing again would strand the
            // prototype cell at a refcount its owners never drop.
            Disp::ObjectGetProtoOf => __torajs_anyv_get_proto_of_any(arg_at(argv, argc, 0)),
            Disp::ObjectSetProtoOf => {
                let obj = arg_at(argv, argc, 0);
                __torajs_anyv_set_prototype_of(obj, arg_at(argv, argc, 1));
                if __torajs_throw_check() != 0 {
                    return VALUE_UNDEFINED;
                }
                own(obj)
            }
            Disp::ObjectFromEntries => __torajs_anyv_from_entries(arg_at(argv, argc, 0)),
        }
    }
}

/// Hand an argv borrow back as the OWNED result the boxed entry's
/// contract promises: every consumer of an any-lane call result drops
/// it, so a static that answers one of its own arguments has to raise
/// the count first (the typed tier does the same at
/// `ssa_lower_call_object_get_prototype_of.rs:112`). Immediates carry
/// no refcount, so only cells inc.
unsafe fn own(v: u64) -> u64 {
    if crate::nanbox::is_cell(v) {
        unsafe { torajs_rc::__torajs_rc_inc(crate::nanbox::as_void_ptr(v)) };
    }
    v
}

/// argv[i], missing → undefined.
unsafe fn arg_at(argv: *const u64, argc: i64, i: i64) -> u64 {
    if i < argc {
        unsafe { *argv.add(i as usize) }
    } else {
        VALUE_UNDEFINED
    }
}

fn box_bool(b: bool) -> u64 {
    if b {
        crate::nanbox::VALUE_TRUE
    } else {
        crate::nanbox::VALUE_FALSE
    }
}

/// §21.1.2 `Number.is*` — non-number input answers false (no
/// coercion); int32 immediates are integral by construction.
fn num_predicate(p: &NumPred, v: u64) -> bool {
    use crate::nanbox::{as_double, is_double, is_int32};
    if is_int32(v) {
        return match p {
            NumPred::Nan => false,
            _ => true,
        };
    }
    if !is_double(v) {
        return false;
    }
    let x = as_double(v);
    match p {
        NumPred::Integer => x.is_finite() && x.trunc() == x,
        NumPred::Nan => x.is_nan(),
        NumPred::Finite => x.is_finite(),
        NumPred::SafeInteger => x.is_finite() && x.trunc() == x && x.abs() <= 9007199254740991.0,
    }
}

/// The interned cell for an ns-static id — lazily allocated,
/// immortal, same closure layout as `builtin_method_cell`.
pub(crate) fn ns_static_cell(id: i64) -> *mut u8 {
    let slot = &NS_CELLS[id as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return p as *mut u8;
    }
    // SAFETY: fresh CELL_SIZE allocation, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(CELL_SIZE, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = FLAG_STATIC_LITERAL;
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = ns_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) = ns_dispatch_entry as *const () as u64;
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = id as u64;
        slot.store(cell as u64, Ordering::Relaxed);
        cell
    }
}

/// Compiler face — the interned cell for a baked table id. The
/// result is a Closure-repr borrow of an immortal cell (rc traffic
/// no-ops on the static flag).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_static_cell(id: i64) -> *mut u8 {
    ns_static_cell(id)
}

/// The ns-static id a cell carries — `None` for every other closure
/// shape (discriminated by the boxed entry's address, the same
/// scheme `builtin_method_mid` uses).
pub(crate) unsafe fn ns_static_id_of(ptr: *const c_void) -> Option<i64> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == ns_dispatch_entry as *const () as u64 {
            Some(*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) as i64)
        } else {
            None
        }
    }
}

/// Reflection name of an ns-static cell (`[Function: max]` /
/// toString native form), `None` for other closures.
pub(crate) unsafe fn ns_static_name(ptr: *const c_void) -> Option<&'static str> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    ns_static_meta(id).map(|r| r.name)
}

/// Interned `.name` Str cell of an ns-static cell.
pub(crate) unsafe fn ns_static_name_cell_of(ptr: *const c_void) -> Option<*mut u8> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    let row = ns_static_meta(id)?;
    let slot = &NS_NAME_CELLS[id as usize];
    let p = slot.load(Ordering::Relaxed);
    if p != 0 {
        return Some(p as *mut u8);
    }
    let cell = mint_immortal_str(row.name.as_bytes());
    slot.store(cell as u64, Ordering::Relaxed);
    Some(cell)
}

/// ES-spec `length` of an ns-static cell.
pub(crate) unsafe fn ns_static_arity(ptr: *const c_void) -> Option<u32> {
    let id = unsafe { ns_static_id_of(ptr) }?;
    ns_static_meta(id).map(|r| r.length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_rc::ns_static::NS_STATIC_TABLE;

    #[test]
    fn dispatch_table_lockstep() {
        assert_eq!(DISPATCH.len(), NS_STATIC_TABLE.len());
        assert!(NS_STATIC_TABLE.len() <= TABLE_SIZE);
    }

    #[test]
    fn to_i64_mod32_edges() {
        assert_eq!(to_i64_mod32(3.7), 3);
        assert_eq!(to_i64_mod32(-1.0), -1);
        assert_eq!(to_i64_mod32(4294967297.0), 1);
        assert_eq!(to_i64_mod32(f64::NAN), 0);
        assert_eq!(to_i64_mod32(f64::INFINITY), 0);
        assert_eq!(
            to_i64_mod32(1e20) as i32,
            (1e20_f64 % 4294967296.0) as i64 as i32
        );
    }

    #[test]
    fn cell_shape_and_probes() {
        let id = torajs_rc::ns_static::ns_static_id("Math", "max");
        let cell = ns_static_cell(id);
        assert_eq!(cell, ns_static_cell(id), "interned identity");
        unsafe {
            assert_eq!(ns_static_id_of(cell as *const c_void), Some(id));
            assert_eq!(ns_static_name(cell as *const c_void), Some("max"));
            assert_eq!(ns_static_arity(cell as *const c_void), Some(2));
            // A mid-keyed method cell never answers the ns probe.
            let mid_cell =
                crate::method_value::builtin_method_cell(torajs_rc::ANY_METHOD_TO_STRING);
            assert_eq!(ns_static_id_of(mid_cell as *const c_void), None);
        }
    }
}
