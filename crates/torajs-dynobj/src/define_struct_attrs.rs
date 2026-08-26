//! The declared-field attribute algebra of a struct receiver's
//! `Object.defineProperty` — split from [`super::define_struct`]
//! (file-size limit): the parent answers "which arm does this define
//! take", this answers "what attribute word results and whether the
//! redefine is admissible".

use crate::layout::{
    BUCKET_FLAG_CONFIGURABLE, BUCKET_FLAG_ENUMERABLE, BUCKET_FLAG_WRITABLE,
    DEFINE_FLAG_CONFIGURABLE, DEFINE_FLAG_ENUMERABLE, DEFINE_FLAG_WRITABLE,
    DEFINE_PRESENT_CONFIGURABLE, DEFINE_PRESENT_ENUMERABLE, DEFINE_PRESENT_VALUE,
    DEFINE_PRESENT_WRITABLE,
};

/// §10.1.6.3 ValidateAndApplyPropertyDescriptor, attribute half, over
/// a (current attributes, descriptor) pair rather than a dict `Entry`
/// — a declared field has no entry to point at.
///
/// A configurable property accepts every change, so only the
/// non-configurable branch has rules: no `configurable` upgrade, no
/// `enumerable` change, and under non-writable no `writable` upgrade
/// and no value change other than SameValue.
pub(crate) fn validate_field_redefine(cur: u64, flags_byte: u64, value_same: bool) -> bool {
    if cur & BUCKET_FLAG_CONFIGURABLE != 0 {
        return true;
    }
    // "asks for X" = the descriptor spells X present AND sets it.
    let asks = |present: u64, flag: u64| flags_byte & present != 0 && flags_byte & flag != 0;
    if asks(DEFINE_PRESENT_CONFIGURABLE, DEFINE_FLAG_CONFIGURABLE) {
        return false;
    }
    if flags_byte & DEFINE_PRESENT_ENUMERABLE != 0
        && (flags_byte & DEFINE_FLAG_ENUMERABLE != 0) != (cur & BUCKET_FLAG_ENUMERABLE != 0)
    {
        return false;
    }
    if cur & BUCKET_FLAG_WRITABLE == 0 {
        if asks(DEFINE_PRESENT_WRITABLE, DEFINE_FLAG_WRITABLE) {
            return false;
        }
        if flags_byte & DEFINE_PRESENT_VALUE != 0 && !value_same {
            return false;
        }
    }
    true
}

/// Re-spell merged bucket attributes as a `flags_byte` for the sidecar
/// write: every attribute PRESENT (so a fresh entry lands on the
/// merged set rather than on the fresh-define default of false), no
/// `[[Value]]`.
pub(crate) fn sidecar_flags_byte(merged: u64) -> u64 {
    let mut out = DEFINE_PRESENT_WRITABLE | DEFINE_PRESENT_ENUMERABLE | DEFINE_PRESENT_CONFIGURABLE;
    if merged & BUCKET_FLAG_WRITABLE != 0 {
        out |= DEFINE_FLAG_WRITABLE;
    }
    if merged & BUCKET_FLAG_ENUMERABLE != 0 {
        out |= DEFINE_FLAG_ENUMERABLE;
    }
    if merged & BUCKET_FLAG_CONFIGURABLE != 0 {
        out |= DEFINE_FLAG_CONFIGURABLE;
    }
    out
}

/// Fold the descriptor's PRESENT attributes over the current ones —
/// §10.1.6.3's "absent fields keep the current value". This is the
/// reason the sidecar cannot simply be written by recursing into
/// `define_apply` with the caller's `flags_byte`: a FRESH dict entry
/// defaults absent attributes to false, whereas a declared field's
/// absent attributes default to its live state.
pub(crate) fn merge_field_attrs(cur: u64, flags_byte: u64) -> u64 {
    let mut out = cur;
    for (present, desc_flag, bucket_flag) in [
        (
            DEFINE_PRESENT_WRITABLE,
            DEFINE_FLAG_WRITABLE,
            BUCKET_FLAG_WRITABLE,
        ),
        (
            DEFINE_PRESENT_ENUMERABLE,
            DEFINE_FLAG_ENUMERABLE,
            BUCKET_FLAG_ENUMERABLE,
        ),
        (
            DEFINE_PRESENT_CONFIGURABLE,
            DEFINE_FLAG_CONFIGURABLE,
            BUCKET_FLAG_CONFIGURABLE,
        ),
    ] {
        if flags_byte & present != 0 {
            if flags_byte & desc_flag != 0 {
                out |= bucket_flag;
            } else {
                out &= !bucket_flag;
            }
        }
    }
    out
}
