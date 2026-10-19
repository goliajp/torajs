//! A settled value read into the form its reader holds.
//!
//! A promise cell's stamp (`value_repr`) is the only runtime record of
//! the form its slot holds, and a reader's form is fixed at compile
//! time: an `await`, a `.then` handler's parameter, a slot of
//! `Promise.all`'s result array, the `value` of an `allSettled` record.
//! Every one of them converts through here, so a cell that settled in
//! another form than the reader's reads the same everywhere.

use crate::layout::{REPR_F64, REPR_I64};

/// The form a slot of form `target` holds a value settled as `src` in:
/// the target when the value converts into it, `src` when it is taken
/// as it is (no target named, or a form that does not convert).
#[inline]
pub(crate) fn read_form(src: u8, target: u8) -> u8 {
    match (src, target) {
        (REPR_I64, REPR_F64) | (REPR_F64, REPR_I64) => target,
        _ => crate::combinator::unbox_target(src, target).unwrap_or(src),
    }
}

/// `v`, settled as `src`, in the form `target`: a box from the any
/// lane is unboxed into the target lane and a number settled at the
/// other width converts. Rc-neutral; a target of `REPR_UNSTAMPED` or
/// `REPR_ANY` takes the slot as it is (boxing is the caller's, since
/// it pays for the box).
///
/// # Safety
/// `v` is a live value of form `src`.
#[inline]
pub(crate) unsafe fn into_form(src: u8, target: u8, v: i64) -> i64 {
    if src == target {
        return v;
    }
    if matches!((src, target), (REPR_I64, REPR_F64) | (REPR_F64, REPR_I64)) {
        return other_width(src, v);
    }
    match crate::combinator::unbox_target(src, target) {
        Some(lane) => unsafe { crate::then_box::unbox_settled(lane, v) },
        None => v,
    }
}

/// A number settled at the other width than the reader's (a promise
/// adopted from one whose resolver had its own) is converted.
///
/// From f64 to i64 this is exact only for an integer, and the compiler
/// keeps it so: a reader asks for i64 only at a value point the width
/// analysis proved integral, and every promise that can reach the
/// point joins its width (a `Promise<number>` slot written from `any`,
/// a field read back, an `allSettled` record's `value`). A fraction
/// here is a value point the analysis missed.
#[cold]
fn other_width(src: u8, v: i64) -> i64 {
    if src == REPR_I64 {
        (v as f64).to_bits() as i64
    } else {
        f64::from_bits(v as u64) as i64
    }
}
