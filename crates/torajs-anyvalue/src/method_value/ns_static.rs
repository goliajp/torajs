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

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::ns_static::ns_static_meta;
use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use crate::nanbox::{VALUE_UNDEFINED, box_double, box_int32};

use super::{
    CELL_SIZE, CLOSURE_BOXED_ENTRY_OFF, CLOSURE_CAP_BASE_OFF, CLOSURE_DROP_FN_OFF,
    CLOSURE_FN_ADDR_OFF, CLOSURE_PROPS_OFF, TABLE_SIZE, mint_immortal_str,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    /// torajs-throw — 1 when a pending throw is recorded (a poisoned
    /// valueOf during ToNumber aborts the remaining coercions).
    fn __torajs_throw_check() -> i64;
    fn __torajs_math_sqrt(x: f64) -> f64;
    fn __torajs_math_abs(x: f64) -> f64;
    fn __torajs_math_floor(x: f64) -> f64;
    fn __torajs_math_ceil(x: f64) -> f64;
    fn __torajs_math_log(x: f64) -> f64;
    fn __torajs_math_exp(x: f64) -> f64;
    fn __torajs_math_sign(x: f64) -> f64;
    fn __torajs_math_round(x: f64) -> f64;
    fn __torajs_math_trunc(x: f64) -> f64;
    fn __torajs_math_sin(x: f64) -> f64;
    fn __torajs_math_cos(x: f64) -> f64;
    fn __torajs_math_tan(x: f64) -> f64;
    fn __torajs_math_asin(x: f64) -> f64;
    fn __torajs_math_acos(x: f64) -> f64;
    fn __torajs_math_atan(x: f64) -> f64;
    fn __torajs_math_log2(x: f64) -> f64;
    fn __torajs_math_log10(x: f64) -> f64;
    fn __torajs_math_cbrt(x: f64) -> f64;
    fn __torajs_math_sinh(x: f64) -> f64;
    fn __torajs_math_cosh(x: f64) -> f64;
    fn __torajs_math_tanh(x: f64) -> f64;
    fn __torajs_math_asinh(x: f64) -> f64;
    fn __torajs_math_acosh(x: f64) -> f64;
    fn __torajs_math_atanh(x: f64) -> f64;
    fn __torajs_math_expm1(x: f64) -> f64;
    fn __torajs_math_log1p(x: f64) -> f64;
    fn __torajs_math_fround(x: f64) -> f64;
    fn __torajs_math_f16round(x: f64) -> f64;
    fn __torajs_math_pow(x: f64, y: f64) -> f64;
    fn __torajs_math_min(x: f64, y: f64) -> f64;
    fn __torajs_math_max(x: f64, y: f64) -> f64;
    fn __torajs_math_atan2(y: f64, x: f64) -> f64;
    fn __torajs_math_imul(a: i64, b: i64) -> i64;
    fn __torajs_math_clz32(x: i64) -> i64;
    fn __torajs_math_random() -> f64;
}

/// Per-id dispatch shape. Index-lockstep with
/// [`torajs_rc::ns_static::NS_STATIC_TABLE`].
enum Disp {
    /// f64 → f64 unary (argc 0 coerces undefined → NaN).
    F(unsafe extern "C" fn(f64) -> f64),
    /// f64 × f64 → f64 binary (missing args coerce to NaN).
    Ff(unsafe extern "C" fn(f64, f64) -> f64),
    /// §21.3.2.24/25 variadic reduction (empty → ±Infinity).
    MinMax { is_max: bool },
    /// ToInt32 pair → i32 result (imul).
    I32Pair(unsafe extern "C" fn(i64, i64) -> i64),
    /// ToUint32 unary → i32-ranged result (clz32).
    I32One(unsafe extern "C" fn(i64) -> i64),
    /// () → f64 (random).
    Nullary(unsafe extern "C" fn() -> f64),
}

static DISPATCH: &[Disp] = &[
    Disp::F(__torajs_math_sqrt),
    Disp::F(__torajs_math_abs),
    Disp::F(__torajs_math_floor),
    Disp::F(__torajs_math_ceil),
    Disp::F(__torajs_math_log),
    Disp::F(__torajs_math_exp),
    Disp::F(__torajs_math_sign),
    Disp::F(__torajs_math_round),
    Disp::F(__torajs_math_trunc),
    Disp::F(__torajs_math_sin),
    Disp::F(__torajs_math_cos),
    Disp::F(__torajs_math_tan),
    Disp::F(__torajs_math_asin),
    Disp::F(__torajs_math_acos),
    Disp::F(__torajs_math_atan),
    Disp::F(__torajs_math_log2),
    Disp::F(__torajs_math_log10),
    Disp::F(__torajs_math_cbrt),
    Disp::F(__torajs_math_sinh),
    Disp::F(__torajs_math_cosh),
    Disp::F(__torajs_math_tanh),
    Disp::F(__torajs_math_asinh),
    Disp::F(__torajs_math_acosh),
    Disp::F(__torajs_math_atanh),
    Disp::F(__torajs_math_expm1),
    Disp::F(__torajs_math_log1p),
    Disp::F(__torajs_math_fround),
    Disp::F(__torajs_math_f16round),
    Disp::Ff(__torajs_math_pow),
    Disp::MinMax { is_max: false },
    Disp::MinMax { is_max: true },
    Disp::Ff(__torajs_math_atan2),
    Disp::I32Pair(__torajs_math_imul),
    Disp::I32One(__torajs_math_clz32),
    Disp::Nullary(__torajs_math_random),
];

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
        }
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
