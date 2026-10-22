//! A settled value read into the form its reader holds.
//!
//! A promise cell's stamp (`value_repr`) is the only runtime record of
//! the form its slot holds, and a reader's form is fixed at compile
//! time: an `await`, a `.then` / `.catch` handler's parameter, a slot
//! of `Promise.all`'s result array, the `value` of an `allSettled`
//! record. Every one of them converts through here, so a cell that
//! settled in another form than the reader's reads the same everywhere.
//!
//! A combinator's result element is one reader only the call site can
//! name — the checker types `Promise.all`'s result `Promise<T[]>`,
//! while SSA's `Type::Promise` is inner-erased — so the site hands that
//! form down as `target_repr`, 0 when it could not name one. Without it
//! an executor-minted cell (`new Promise((res) => res(v))`), which
//! settles through the any lane and stamps `REPR_ANY`, left box bits in
//! a typed result slot, and `a[0] + a[1]` read them as f64 and
//! answered NaN.

use crate::layout::{REPR_ANY, REPR_F64, REPR_I64, REPR_UNSTAMPED};

/// Does a value settled as `src` convert into a reader of form
/// `target`? Not when the two agree, nor when either side names no
/// form, nor for an `any` reader (boxing is the caller's, since it
/// pays for the box).
#[inline]
pub(crate) fn converts(src: u8, target: u8) -> bool {
    src != target && src != REPR_UNSTAMPED && target != REPR_UNSTAMPED && target != REPR_ANY
}

/// The form a slot of form `target` holds a value settled as `src` in.
#[inline]
pub(crate) fn read_form(src: u8, target: u8) -> u8 {
    if converts(src, target) { target } else { src }
}

/// The form a result array holds when the call site named none and
/// its elements settled as `held` so far and now `src`: the first
/// element's, except that integers and fractions meet at f64, since an
/// i64 slot cannot hold a fraction.
#[inline]
pub(crate) fn join_form(held: u8, src: u8) -> u8 {
    match (held, src) {
        (REPR_UNSTAMPED, _) => src,
        (REPR_I64, REPR_F64) => REPR_F64,
        _ => held,
    }
}

/// [`join_form`] over the settled elements of the promise array `arr`
/// (`len` slots), from the first one's form `first`.
///
/// # Safety
/// `arr` is a live array whose slots are NULL or live Promise cells.
#[cold]
pub(crate) unsafe fn joined_form(arr: *mut core::ffi::c_void, len: u64, first: u8) -> u8 {
    let mut form = first;
    for i in 0..len {
        let pp = unsafe { crate::combinator::arr_slot_ptr(arr, i) };
        if !pp.is_null() {
            form = join_form(form, unsafe { (*pp).value_repr });
        }
    }
    form
}

/// `v`, settled as `src`, in the form `target`: a box from the any
/// lane is unboxed into the target lane, a number settled at the other
/// width converts, and any other typed pair goes through a box (by
/// what is stored, unboxed by what is read). Rc-neutral.
///
/// # Safety
/// `v` is a live value of form `src`.
#[inline(always)]
pub(crate) unsafe fn into_form(src: u8, target: u8, v: i64) -> i64 {
    if src == target {
        v
    } else {
        unsafe { convert(src, target, v) }
    }
}

#[cold]
#[inline(never)]
pub(crate) unsafe fn convert(src: u8, target: u8, v: i64) -> i64 {
    if !converts(src, target) {
        return v;
    }
    unsafe {
        match (src, target) {
            (REPR_I64, REPR_F64) | (REPR_F64, REPR_I64) => other_width(src, v),
            (REPR_ANY, _) => crate::then_box::unbox_settled(target, v),
            _ => crate::then_box::unbox_settled(target, crate::then_box::box_settled(src, v)),
        }
    }
}

/// A number settled at the other width than the reader's (a promise
/// adopted from one whose resolver had its own) is converted.
///
/// From f64 to i64 this is exact only for an integer, and the compiler
/// keeps it so: a reader asks for i64 only at a value point the width
/// analysis proved integral, which it does only when every promise
/// that can reach the point is one it follows (a point a promise from
/// `any` can reach reads at f64). A fraction here is a value point the
/// analysis missed; a debug build stops on it.
pub(crate) fn other_width(src: u8, v: i64) -> i64 {
    if src == REPR_I64 {
        (v as f64).to_bits() as i64
    } else {
        exact_i64(f64::from_bits(v as u64))
    }
}

/// `f` read by an i64 reader, which the width analysis gives only a
/// number it proved integral; a debug build stops on anything an i64
/// cannot hold as it is: a fraction, NaN, an infinity, or -0.
#[inline]
pub(crate) fn exact_i64(f: f64) -> i64 {
    debug_assert!(
        f.is_finite() && f.fract() == 0.0 && !(f == 0.0 && f.is_sign_negative()),
        "a number an i64 cannot hold read at i64"
    );
    f as i64
}
