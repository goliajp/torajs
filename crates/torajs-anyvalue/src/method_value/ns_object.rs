//! RFC 20260801-ns-object-value — the `Math` NAMESPACE OBJECT as a
//! first-class value.
//!
//! `Math` read in a value position (thisArg / call arg / return /
//! `this === Math` identity) answers ONE immortal dynobj singleton,
//! lazily minted and pre-filled with every Math static the
//! ns-static value face already interns (RFC 20260719 — the same
//! cells `Math.max`-as-a-value answers, so a post-escape dynamic
//! call rides the existing boxed dual entry) plus the eight
//! §21.3.1 value properties. Being a real dynobj, member reads,
//! expando writes, delete, freeze and for-in all take the ordinary
//! dynobj lanes with zero new dispatch surface.
//!
//! `Object.prototype.toString.call(Math)` answers "[object Math]"
//! per §21.3.1.9 through the pointer-identity probe below (the
//! badge walk in `method_call_object_proto::cell_badge` checks it
//! before the generic dynobj fallback).

use core::ffi::c_void;
use core::sync::atomic::{AtomicU64, Ordering};

use torajs_rc::ns_static::NS_STATIC_TABLE;
use torajs_rc::{AnySlotTag, FLAG_STATIC_LITERAL};

use crate::AnyValue;
use crate::nanbox::box_void_ptr;

use super::mint_immortal_str;
use super::ns_static::ns_static_cell;

unsafe extern "C" {
    fn __torajs_dynobj_alloc() -> *mut c_void;
    fn __torajs_dynobj_set(obj_slot: *mut *mut c_void, key: *mut c_void, tag: u64, value: u64);
}

/// The interned Math singleton — 0 until first minted.
static MATH_OBJECT: AtomicU64 = AtomicU64::new(0);

/// §21.3.1 value properties (spec order).
const MATH_CONSTS: &[(&[u8], f64)] = &[
    (b"E", core::f64::consts::E),
    (b"LN10", core::f64::consts::LN_10),
    (b"LN2", core::f64::consts::LN_2),
    (b"LOG10E", core::f64::consts::LOG10_E),
    (b"LOG2E", core::f64::consts::LOG2_E),
    (b"PI", core::f64::consts::PI),
    (b"SQRT1_2", core::f64::consts::FRAC_1_SQRT_2),
    (b"SQRT2", core::f64::consts::SQRT_2),
];

/// The Math singleton's dynobj pointer — mints on first use. The
/// compare-exchange keeps identity unique under concurrent first
/// touches (multi-thread-ready shape; the loser leaks one
/// pre-filled object, which the immortal flag makes inert).
pub(crate) fn math_object_ptr() -> *mut c_void {
    let p = MATH_OBJECT.load(Ordering::Acquire);
    if p != 0 {
        return p as *mut c_void;
    }
    // SAFETY: fresh dynobj, filled with immortal cells (the interned
    // ns-static closures carry FLAG_STATIC_LITERAL; the F64 consts
    // are inline slots); the object itself takes the static flag so
    // rc traffic no-ops and scope drops never reclaim it.
    unsafe {
        let mut obj = __torajs_dynobj_alloc();
        for (id, row) in NS_STATIC_TABLE.iter().enumerate() {
            if row.ns != "Math" {
                continue;
            }
            let cell = ns_static_cell(id as i64);
            let key = mint_immortal_str(row.name.as_bytes());
            __torajs_dynobj_set(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::Heap as u64,
                cell as u64,
            );
        }
        for (name, v) in MATH_CONSTS {
            let key = mint_immortal_str(name);
            __torajs_dynobj_set(
                &mut obj,
                key as *mut c_void,
                AnySlotTag::F64 as u64,
                v.to_bits(),
            );
        }
        *(obj.cast::<u8>().add(6) as *mut u16) |= FLAG_STATIC_LITERAL;
        match MATH_OBJECT.compare_exchange(0, obj as u64, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => obj,
            Err(winner) => winner as *mut c_void,
        }
    }
}

/// Compiler face — `Math` lowered in a value position.
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_ns_object_math() -> AnyValue {
    box_void_ptr(math_object_ptr())
}

/// Identity probe for the toString badge — true only for the minted
/// singleton (0-compare before any mint, so a plain dynobj never
/// false-positives).
pub(crate) fn is_math_object(ptr: *const c_void) -> bool {
    let p = MATH_OBJECT.load(Ordering::Relaxed);
    p != 0 && p == ptr as u64
}
