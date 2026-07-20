//! Builtin-proto family axis of the method-cell intern (RFC
//! 20260721-string-proto-cluster 刀 3) — the family constants, the
//! cell-side family decode, and the receiver-shape → family map.
//! The intern table itself stays in the parent module (it packs
//! family and mid into one capture slot at mint time).

use core::ffi::c_void;

use torajs_rc::Tag;

use super::CLOSURE_CAP_BASE_OFF;
use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};

/// One intern row per builtin-proto family (torajs-rc
/// `builtin_proto.rs` tags, Number=0 … Function=13) plus row 0 for
/// family-less mints (the extern staticlib face / size getters).
/// ES §8.3: each prototype's method is a DISTINCT function object —
/// `String.prototype.concat` and `Array.prototype.concat` must not
/// intern to the same cell (a shared-mid cell made the `.call`
/// generic-coerce lane undecidable: G4).
pub(crate) const FAMILY_ROWS: usize = torajs_rc::builtin_proto::NUM_BUILTIN_PROTOS + 1;

/// The String family's proto tag — the one family whose reified
/// methods run the §22.1.3 generic ToString(this) coerce on `.call`
/// / borrow re-dispatch.
pub(crate) const STR_PROTO_FAMILY: i64 = 3;

/// The builtin-proto family a reified cell was minted for — -1 for
/// a family-less mint. Callers gate on
/// [`super::builtin_method_mid`] being `Some` first (this reads the
/// capture slot unconditionally).
pub(crate) unsafe fn builtin_method_family(ptr: *mut c_void) -> i64 {
    unsafe { ((*(ptr.cast::<u8>().add(CLOSURE_CAP_BASE_OFF) as *const u64) >> 32) as i64) - 1 }
}

/// The builtin-proto family a receiver's method reads resolve
/// against — the value-read twin of the per-tag dispatch ladder.
/// -1 = no builtin-proto family (the read stays family-less).
pub(crate) fn recv_proto_family(recv: AnyValue) -> i64 {
    if is_short_str(recv) {
        return STR_PROTO_FAMILY;
    }
    if is_bool(recv) {
        return 4;
    }
    if is_int32(recv) || is_double(recv) {
        return 0;
    }
    if !is_cell(recv) {
        return -1;
    }
    let ptr = as_void_ptr(recv);
    // SAFETY: is_cell guarantees a live heap pointer with a header.
    let tag = unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() };
    match tag {
        t if t == Tag::Str as u16 => STR_PROTO_FAMILY,
        t if t == Tag::Obj as u16 || t == Tag::DynObj as u16 => 1,
        t if t == Tag::Arr as u16 => 2,
        t if t == Tag::Closure as u16 => 13,
        t if t == Tag::RegExp as u16 => 7,
        t if t == Tag::Date as u16 => 8,
        t if t == Tag::Symbol as u16 => 5,
        t if t == Tag::Promise as u16 => 10,
        t if t == Tag::BigInt as u16 => 6,
        t if t == Tag::Map as u16 => 11,
        t if t == Tag::Set as u16 => 12,
        t if t == Tag::NumberWrapper as u16 => 0,
        t if t == Tag::StringWrapper as u16 => STR_PROTO_FAMILY,
        t if t == Tag::BooleanWrapper as u16 => 4,
        _ => -1,
    }
}
