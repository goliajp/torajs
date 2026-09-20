//! The `This<Type>Value` receiver shapes — §21.1.3 thisNumberValue,
//! §20.3.3 thisBooleanValue, §22.1.3 thisStringValue and the
//! ToObject-wrapper probe that rides with them.
//!
//! Split out of [`crate::method_call_closure_generic`] under the
//! 500-line file cap. Verbatim move; these four answer one question
//! each and the gate next door reads them.
//!
//! Each abstract operation admits the primitive AND its wrapper
//! object, because the wrapper's internal data slot is what the
//! operation reads — `Number.prototype.toString.call(new Number(1))`
//! is 1, not a TypeError.

use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};
use torajs_rc::Tag;

/// thisNumberValue shape — a number immediate or a Number wrapper
/// (whose thisNumberValue is its [[NumberData]]).
pub(crate) fn is_number_shaped(v: AnyValue) -> bool {
    if is_int32(v) || is_double(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::NumberWrapper as u16
}

/// thisBooleanValue shape — a bool immediate or a Boolean wrapper.
pub(crate) fn is_boolean_shaped(v: AnyValue) -> bool {
    if is_bool(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::BooleanWrapper as u16
}

/// A primitive shape whose ToObject mints a fresh wrapper — the
/// receivers whose own dispatch arm would answer the WRONG concat
/// (string concat on Str shapes, no-such on bool/number). Heap
/// receivers (wrapper objects included) already ride the cell arm's
/// seeded concat gate.
pub(crate) fn is_prim_shaped(v: AnyValue) -> bool {
    if is_bool(v) || is_int32(v) || is_double(v) || is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16
}

/// §6.1.4-shape probe for the thisStringValue gate — a ShortStr
/// immediate, a Str cell, or a String wrapper object (whose
/// thisStringValue is its [[StringData]]).
pub(crate) fn is_string_shaped(v: AnyValue) -> bool {
    if is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16 || tag == Tag::StringWrapper as u16
}

/// ThisBigIntValue shape — a BigInt cell. There is no BigInt wrapper
/// object to admit here: `to_object.rs` records that gap in its own
/// words ("a BigInt cell falls through to identity"), so the day one
/// exists this predicate is where it joins.
pub(crate) fn is_bigint_shaped(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::BigInt as u16
}

/// §20.2.3.5 step 4's subject — `Function.prototype.toString` is the
/// one method on this list whose brand is "callable" rather than a
/// primitive's internal data slot.
pub(crate) fn is_callable_shaped(v: AnyValue) -> bool {
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Closure as u16
}
