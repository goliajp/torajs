//! The `This<Type>Value` receiver shapes — §21.1.3 thisNumberValue,
//! §20.3.3 thisBooleanValue, §22.1.3 thisStringValue and the
//! ToObject-wrapper probe that rides with them.
//!
//! Split out of [`crate::method_call_closure_generic`] under the
//! 500-line file cap. Verbatim move; these four answer one question
//! each and the gate next door reads them.
//!
//! Each abstract operation admits the primitive, its wrapper object
//! and the builtin prototype singleton, because the wrapper's internal data slot is what the
//! operation reads — `Number.prototype.toString.call(new Number(1))`
//! is 1, not a TypeError.

use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};
use torajs_rc::Tag;
use torajs_rc::builtin_proto::{
    __torajs_peek_builtin_prototype, BOOLEAN_PROTO_TAG, NUMBER_PROTO_TAG, STRING_PROTO_TAG,
};

/// Is `v` the `<Ctor>.prototype` singleton of builtin `proto_tag`? The
/// Number / Boolean / String prototypes are themselves ordinary
/// objects WITH the primitive's internal slot (§21.1.3 +0, §20.3.3
/// false, §22.1.3 the empty string), so each this-value operation
/// admits its own prototype.
fn is_proto_singleton(v: AnyValue, proto_tag: usize) -> bool {
    // SAFETY: the peek only reads an atomic slot; NULL (never minted)
    // cannot equal a live cell.
    let proto = unsafe { __torajs_peek_builtin_prototype(proto_tag as i64) };
    !proto.is_null() && as_void_ptr(v) == proto
}

/// The internal slot value of the Number / Boolean / String prototype
/// singleton, when `v` is the one whose family `fam` names — what the
/// family's this-value operation reads off it (+0, false, "").
pub(crate) fn proto_singleton_data(v: AnyValue, fam: i64) -> Option<AnyValue> {
    use crate::method_value::{BOOL_PROTO_FAMILY, NUM_PROTO_FAMILY, STR_PROTO_FAMILY};
    if !is_cell(v) {
        return None;
    }
    if fam == NUM_PROTO_FAMILY && is_proto_singleton(v, NUMBER_PROTO_TAG) {
        return Some(crate::nanbox::box_int32(0));
    }
    if fam == BOOL_PROTO_FAMILY && is_proto_singleton(v, BOOLEAN_PROTO_TAG) {
        return Some(crate::nanbox::VALUE_FALSE);
    }
    if fam == STR_PROTO_FAMILY && is_proto_singleton(v, STRING_PROTO_TAG) {
        return crate::nanbox::try_box_short_str(b"");
    }
    None
}

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
    tag == Tag::NumberWrapper as u16 || is_proto_singleton(v, NUMBER_PROTO_TAG)
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
    tag == Tag::BooleanWrapper as u16 || is_proto_singleton(v, BOOLEAN_PROTO_TAG)
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
    tag == Tag::Str as u16
        || tag == Tag::StringWrapper as u16
        || is_proto_singleton(v, STRING_PROTO_TAG)
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

// §20.2.3.5 step 4's subject is IsCallable, not a primitive's
// internal data slot, so it is NOT a shape predicate and does not
// live here — the gate next door reads `construct::is_callable`
// (ES §7.2.3) directly. A copy did live here for one rotation and
// listed only `Tag::Closure`, which made `String(Error)` throw:
// a builtin constructor read as a value is a `__class_<C>` dynobj.
