//! Shared per-arg helpers of the ns-static dispatch family
//! ([`super::ns_static`]) — arg coercion / boxing / the `Number.is*`
//! predicate core. Split out of `ns_static.rs` (rotation 268 —
//! Reflect.set 前的余量腾挪, mechanical move).

use super::ns_static_table::{__torajs_throw_check, NumPred};
use crate::nanbox::VALUE_UNDEFINED;

/// Separator/newline byte between the tag-aware per-arg prints —
/// the Rust-path call (torajs-io is a real Cargo dep) keeps the
/// symbol linked in test binaries, where an extern-only reference
/// leaves the rlib member dead-stripped.
pub(super) fn putc_out(c: u8) {
    torajs_io::__torajs_io_putc_out(i32::from(c));
}

/// ToNumber(argv[i]) with the spec's abrupt-completion contract: a
/// pending throw recorded during coercion (poisoned valueOf) aborts
/// the caller's remaining coercions. Missing args coerce undefined.
pub(super) unsafe fn arg_num(argv: *const u64, argc: i64, i: i64) -> Result<f64, ()> {
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
pub(super) fn to_i64_mod32(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    (x.trunc() % 4294967296.0) as i64
}

pub(super) unsafe fn own(v: u64) -> u64 {
    if crate::nanbox::is_cell(v) {
        unsafe { torajs_rc::__torajs_rc_inc(crate::nanbox::as_void_ptr(v)) };
    }
    v
}

/// argv[i], missing → undefined.
pub(super) unsafe fn arg_at(argv: *const u64, argc: i64, i: i64) -> u64 {
    if i < argc {
        unsafe { *argv.add(i as usize) }
    } else {
        VALUE_UNDEFINED
    }
}

pub(super) fn box_bool(b: bool) -> u64 {
    if b {
        crate::nanbox::VALUE_TRUE
    } else {
        crate::nanbox::VALUE_FALSE
    }
}

/// §21.1.2 `Number.is*` — non-number input answers false (no
/// coercion); int32 immediates are integral by construction.
pub(super) fn num_predicate(p: &NumPred, v: u64) -> bool {
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
