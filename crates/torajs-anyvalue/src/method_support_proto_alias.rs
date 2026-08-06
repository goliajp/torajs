//! Which cell a `<Ctor>.prototype.<name>` read hands out — the
//! owner-resolution + per-prototype alias half of
//! `method_support_proto.rs`, moved out when that file reached the
//! 500-line limit (rotation 314). The support / own / accessor
//! probes stayed behind; this file answers only "whose cell, under
//! which id".

use torajs_rc::{ANY_METHOD_KEYS, ANY_METHOD_TO_STRING, ANY_METHOD_VALUE_OF, ANY_METHOD_VALUES};

/// The (family row, mid) a `<Ctor>.prototype.<name>` read hands its
/// cell out of — resolve the OWNER first, then that owner's alias.
///
/// Both halves matter and neither is the reading prototype's own
/// tag. `Map.prototype.toString` is inherited, so its owner is
/// `Object.prototype` (§20.1.3.6) and the id it must carry is the
/// badge classifier's — asking the alias with the reading tag left
/// it on the generic per-receiver id, which minted a second cell
/// and broke `Map.prototype.toString === Object.prototype.toString`.
/// For a method the family genuinely owns the owner IS the tag, so
/// the three aliases below read exactly as they did.
pub(crate) fn proto_cell_key(tag: i64, mid: i64) -> (i64, i64) {
    let owner = crate::method_value::family::intern_family(tag, mid);
    (owner, set_keys_alias(owner, mid))
}

/// Per-prototype cell aliases — a name-interned mid that a given
/// `<Ctor>.prototype` hands out as a DIFFERENT function object:
/// - §24.2.4.8 `Set.prototype.keys` IS the values function (tag 12,
///   `torajs-rc/builtin_proto.rs` order), so the two compare `===`.
/// - §20.1.3.6 `Object.prototype.toString` (tag 1) is the
///   "[object X]" badge classifier, distinct from every
///   per-receiver `toString` (RFC 20260713-array-proto-residual
///   blade 2).
///
/// Callers reach this through [`proto_cell_key`], which resolves the
/// owning family first — the `tag` here is always the OWNER's.
fn set_keys_alias(tag: i64, mid: i64) -> i64 {
    if tag == 12 && mid == ANY_METHOD_KEYS {
        return ANY_METHOD_VALUES;
    }
    if tag == 1 && mid == ANY_METHOD_TO_STRING {
        return torajs_rc::ANY_METHOD_OBJECT_TO_STRING;
    }
    // §23.1.3.36 — `Array.prototype.toString` (tag 2) is the
    // join-or-badge function, distinct from the shared per-receiver
    // TO_STRING the primitive fast arms answer natively (RFC
    // 20260721 刀 11 G12).
    if tag == 2 && mid == ANY_METHOD_TO_STRING {
        return torajs_rc::ANY_METHOD_ARR_TO_STRING;
    }
    // §20.4.3.3/.4 — `Symbol.prototype`'s (tag 5) toString / valueOf
    // run thisSymbolValue: a non-Symbol receiver throws, so the
    // reified cells carry dedicated ids (the generic ids would
    // re-dispatch `.call(0)` into the number arm and answer 0).
    if tag == 5 && mid == ANY_METHOD_TO_STRING {
        return torajs_rc::ANY_METHOD_SYMBOL_TO_STRING;
    }
    if tag == 5 && mid == ANY_METHOD_VALUE_OF {
        return torajs_rc::ANY_METHOD_SYMBOL_VALUE_OF;
    }
    mid
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_rc::ANY_METHOD_TO_LOCALE_STRING;

    /// The cell a read hands out belongs to the OWNING prototype,
    /// and carries that owner's alias — an inherited `toString` is
    /// `Object.prototype`'s badge classifier, an owned one is not.
    #[test]
    fn inherited_reads_resolve_to_the_owner_cell() {
        assert_eq!(
            proto_cell_key(11, ANY_METHOD_TO_STRING),
            (1, torajs_rc::ANY_METHOD_OBJECT_TO_STRING)
        );
        assert_eq!(
            proto_cell_key(1, ANY_METHOD_TO_STRING),
            (1, torajs_rc::ANY_METHOD_OBJECT_TO_STRING)
        );
        // Owned ones keep both halves: Array's join-or-badge
        // toString (§23.1.3.36) and Set's keys-IS-values alias
        // (§24.2.4.8).
        assert_eq!(
            proto_cell_key(2, ANY_METHOD_TO_STRING),
            (2, torajs_rc::ANY_METHOD_ARR_TO_STRING)
        );
        assert_eq!(proto_cell_key(12, ANY_METHOD_KEYS), (12, ANY_METHOD_VALUES));
        // An inherited toLocaleString has no alias, only an owner.
        assert_eq!(
            proto_cell_key(11, ANY_METHOD_TO_LOCALE_STRING),
            (1, ANY_METHOD_TO_LOCALE_STRING)
        );
    }
}
