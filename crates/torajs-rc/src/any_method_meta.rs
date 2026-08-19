//! Method id → reflection metadata for `any`-receiver method cells
//! — split out of `any_method.rs` by the 500-line file discipline
//! (the id constants + intern tables stay there; this sibling holds
//! the `(name, length)` reflection row per id).

// The meta table references every ANY_METHOD_* constant — a glob
// import keeps the sibling in lockstep as ids append.
use crate::any_method::*;
use crate::any_method_date::*;

/// Method id → `(canonical name, ES-spec "length")` reflection
/// metadata (chunk 715) — the `.name` / `.length` reads off a
/// reified method cell (`method_value`). `None` for
/// [`ANY_METHOD_UNKNOWN`] and out-of-table ids.
///
/// Lengths are the spec `length` property values (expected argument
/// counts, ES2024). One interned id can serve several prototypes;
/// the only length divergence in the table is `toString`
/// (Number.prototype 1 vs String/Boolean/Date/RegExp 0), so this
/// per-mid row carries the majority value 0. Callers that know which
/// prototype the cell was minted for use [`any_method_meta_for`],
/// which resolves that one divergence.
pub fn any_method_meta(mid: i64) -> Option<(&'static str, u32)> {
    Some(match mid {
        ANY_METHOD_PUSH => ("push", 1),
        ANY_METHOD_POP => ("pop", 0),
        ANY_METHOD_CHAR_AT => ("charAt", 1),
        ANY_METHOD_TO_UPPER_CASE => ("toUpperCase", 0),
        ANY_METHOD_TO_LOWER_CASE => ("toLowerCase", 0),
        ANY_METHOD_INDEX_OF => ("indexOf", 1),
        ANY_METHOD_INCLUDES => ("includes", 1),
        ANY_METHOD_SLICE => ("slice", 2),
        ANY_METHOD_SPLIT => ("split", 2),
        ANY_METHOD_TRIM => ("trim", 0),
        ANY_METHOD_TRIM_START => ("trimStart", 0),
        ANY_METHOD_TRIM_END => ("trimEnd", 0),
        ANY_METHOD_SHIFT => ("shift", 0),
        ANY_METHOD_UNSHIFT => ("unshift", 1),
        ANY_METHOD_JOIN => ("join", 1),
        ANY_METHOD_MAP => ("map", 1),
        ANY_METHOD_FILTER => ("filter", 1),
        ANY_METHOD_FOR_EACH => ("forEach", 1),
        ANY_METHOD_GET => ("get", 1),
        ANY_METHOD_SET => ("set", 2),
        ANY_METHOD_HAS => ("has", 1),
        ANY_METHOD_DELETE => ("delete", 1),
        ANY_METHOD_ADD => ("add", 1),
        ANY_METHOD_CLEAR => ("clear", 0),
        ANY_METHOD_GET_TIME => ("getTime", 0),
        ANY_METHOD_VALUE_OF => ("valueOf", 0),
        ANY_METHOD_TO_ISO_STRING => ("toISOString", 0),
        ANY_METHOD_TO_JSON => ("toJSON", 1),
        ANY_METHOD_GET_FULL_YEAR => ("getFullYear", 0),
        ANY_METHOD_GET_UTC_FULL_YEAR => ("getUTCFullYear", 0),
        ANY_METHOD_GET_MONTH => ("getMonth", 0),
        ANY_METHOD_GET_UTC_MONTH => ("getUTCMonth", 0),
        ANY_METHOD_GET_DATE => ("getDate", 0),
        ANY_METHOD_GET_UTC_DATE => ("getUTCDate", 0),
        ANY_METHOD_GET_HOURS => ("getHours", 0),
        ANY_METHOD_GET_UTC_HOURS => ("getUTCHours", 0),
        ANY_METHOD_GET_MINUTES => ("getMinutes", 0),
        ANY_METHOD_GET_UTC_MINUTES => ("getUTCMinutes", 0),
        ANY_METHOD_GET_SECONDS => ("getSeconds", 0),
        ANY_METHOD_GET_UTC_SECONDS => ("getUTCSeconds", 0),
        ANY_METHOD_GET_MILLISECONDS => ("getMilliseconds", 0),
        ANY_METHOD_GET_UTC_MILLISECONDS => ("getUTCMilliseconds", 0),
        ANY_METHOD_GET_DAY => ("getDay", 0),
        ANY_METHOD_GET_UTC_DAY => ("getUTCDay", 0),
        ANY_METHOD_GET_TIMEZONE_OFFSET => ("getTimezoneOffset", 0),
        ANY_METHOD_SET_TIME => ("setTime", 1),
        ANY_METHOD_SET_YEAR => ("setYear", 1),
        ANY_METHOD_GET_YEAR => ("getYear", 0),
        ANY_METHOD_TO_GMT_STRING => ("toGMTString", 0),
        ANY_METHOD_TO_UTC_STRING => ("toUTCString", 0),
        ANY_METHOD_TO_DATE_STRING => ("toDateString", 0),
        ANY_METHOD_TO_TIME_STRING => ("toTimeString", 0),
        ANY_METHOD_GET_OR_INSERT => ("getOrInsert", 2),
        ANY_METHOD_GET_OR_INSERT_COMPUTED => ("getOrInsertComputed", 2),
        ANY_METHOD_TO_LOCALE_STRING => ("toLocaleString", 0),
        ANY_METHOD_TO_LOCALE_DATE_STRING => ("toLocaleDateString", 0),
        ANY_METHOD_TO_LOCALE_TIME_STRING => ("toLocaleTimeString", 0),
        ANY_METHOD_SET_FULL_YEAR => ("setFullYear", 3),
        ANY_METHOD_SET_MONTH => ("setMonth", 2),
        ANY_METHOD_SET_DATE => ("setDate", 1),
        ANY_METHOD_SET_HOURS => ("setHours", 4),
        ANY_METHOD_SET_MINUTES => ("setMinutes", 3),
        ANY_METHOD_SET_SECONDS => ("setSeconds", 2),
        ANY_METHOD_SET_MILLISECONDS => ("setMilliseconds", 1),
        ANY_METHOD_SET_UTC_FULL_YEAR => ("setUTCFullYear", 3),
        ANY_METHOD_SET_UTC_MONTH => ("setUTCMonth", 2),
        ANY_METHOD_SET_UTC_DATE => ("setUTCDate", 1),
        ANY_METHOD_SET_UTC_HOURS => ("setUTCHours", 4),
        ANY_METHOD_SET_UTC_MINUTES => ("setUTCMinutes", 3),
        ANY_METHOD_SET_UTC_SECONDS => ("setUTCSeconds", 2),
        ANY_METHOD_SET_UTC_MILLISECONDS => ("setUTCMilliseconds", 1),
        ANY_METHOD_TO_STRING => ("toString", 0),
        ANY_METHOD_TO_FIXED => ("toFixed", 1),
        ANY_METHOD_TO_EXPONENTIAL => ("toExponential", 1),
        ANY_METHOD_TO_PRECISION => ("toPrecision", 1),
        ANY_METHOD_TEST => ("test", 1),
        ANY_METHOD_EXEC => ("exec", 1),
        ANY_METHOD_THEN => ("then", 2),
        ANY_METHOD_CATCH => ("catch", 1),
        ANY_METHOD_FINALLY => ("finally", 1),
        ANY_METHOD_KEYS => ("keys", 0),
        ANY_METHOD_VALUES => ("values", 0),
        ANY_METHOD_ENTRIES => ("entries", 0),
        ANY_METHOD_NEXT => ("next", 0),
        ANY_METHOD_MATCH => ("match", 1),
        ANY_METHOD_REPLACE => ("replace", 2),
        ANY_METHOD_REPLACE_ALL => ("replaceAll", 2),
        ANY_METHOD_STARTS_WITH => ("startsWith", 1),
        ANY_METHOD_ENDS_WITH => ("endsWith", 1),
        ANY_METHOD_CALL => ("call", 1),
        ANY_METHOD_APPLY => ("apply", 2),
        ANY_METHOD_BIND => ("bind", 1),
        ANY_METHOD_HAS_OWN_PROPERTY => ("hasOwnProperty", 1),
        ANY_METHOD_IS_PROTOTYPE_OF => ("isPrototypeOf", 1),
        ANY_METHOD_PROPERTY_IS_ENUMERABLE => ("propertyIsEnumerable", 1),
        ANY_METHOD_DEFINE_GETTER => ("__defineGetter__", 2),
        ANY_METHOD_DEFINE_SETTER => ("__defineSetter__", 2),
        ANY_METHOD_LOOKUP_GETTER => ("__lookupGetter__", 1),
        ANY_METHOD_LOOKUP_SETTER => ("__lookupSetter__", 1),
        ANY_METHOD_LAST_INDEX_OF => ("lastIndexOf", 1),
        ANY_METHOD_REVERSE => ("reverse", 0),
        ANY_METHOD_CONCAT => ("concat", 1),
        ANY_METHOD_FILL => ("fill", 1),
        ANY_METHOD_COPY_WITHIN => ("copyWithin", 2),
        ANY_METHOD_SPLICE => ("splice", 2),
        ANY_METHOD_EVERY => ("every", 1),
        ANY_METHOD_SOME => ("some", 1),
        ANY_METHOD_FIND => ("find", 1),
        ANY_METHOD_FIND_INDEX => ("findIndex", 1),
        ANY_METHOD_REDUCE => ("reduce", 1),
        ANY_METHOD_REDUCE_RIGHT => ("reduceRight", 1),
        ANY_METHOD_SORT => ("sort", 1),
        ANY_METHOD_ANCHOR => ("anchor", 1),
        ANY_METHOD_FONTCOLOR => ("fontcolor", 1),
        ANY_METHOD_FONTSIZE => ("fontsize", 1),
        ANY_METHOD_LINK => ("link", 1),
        ANY_METHOD_BIG => ("big", 0),
        ANY_METHOD_BLINK => ("blink", 0),
        ANY_METHOD_BOLD => ("bold", 0),
        ANY_METHOD_FIXED => ("fixed", 0),
        ANY_METHOD_ITALICS => ("italics", 0),
        ANY_METHOD_SMALL => ("small", 0),
        ANY_METHOD_STRIKE => ("strike", 0),
        ANY_METHOD_SUB => ("sub", 0),
        ANY_METHOD_SUP => ("sup", 0),
        ANY_METHOD_SUBSTRING => ("substring", 2),
        ANY_METHOD_SUBSTR => ("substr", 2),
        ANY_METHOD_AT => ("at", 1),
        ANY_METHOD_CHAR_CODE_AT => ("charCodeAt", 1),
        ANY_METHOD_PAD_START => ("padStart", 1),
        ANY_METHOD_PAD_END => ("padEnd", 1),
        ANY_METHOD_REPEAT => ("repeat", 1),
        ANY_METHOD_CODE_POINT_AT => ("codePointAt", 1),
        ANY_METHOD_LOCALE_COMPARE => ("localeCompare", 1),
        ANY_METHOD_NORMALIZE => ("normalize", 0),
        ANY_METHOD_TO_LOCALE_LOWER_CASE => ("toLocaleLowerCase", 0),
        ANY_METHOD_TO_LOCALE_UPPER_CASE => ("toLocaleUpperCase", 0),
        ANY_METHOD_SEARCH => ("search", 1),
        ANY_METHOD_MATCH_ALL => ("matchAll", 1),
        ANY_METHOD_IS_WELL_FORMED => ("isWellFormed", 0),
        ANY_METHOD_TO_WELL_FORMED => ("toWellFormed", 0),
        ANY_METHOD_FLAT => ("flat", 0),
        ANY_METHOD_FLAT_MAP => ("flatMap", 1),
        ANY_METHOD_FIND_LAST => ("findLast", 1),
        ANY_METHOD_FIND_LAST_INDEX => ("findLastIndex", 1),
        ANY_METHOD_TO_REVERSED => ("toReversed", 0),
        ANY_METHOD_TO_SORTED => ("toSorted", 1),
        ANY_METHOD_TO_SPLICED => ("toSpliced", 2),
        ANY_METHOD_WITH => ("with", 2),
        ANY_METHOD_UNION => ("union", 1),
        ANY_METHOD_INTERSECTION => ("intersection", 1),
        ANY_METHOD_DIFFERENCE => ("difference", 1),
        ANY_METHOD_SYMMETRIC_DIFFERENCE => ("symmetricDifference", 1),
        ANY_METHOD_IS_SUBSET_OF => ("isSubsetOf", 1),
        ANY_METHOD_IS_SUPERSET_OF => ("isSupersetOf", 1),
        ANY_METHOD_IS_DISJOINT_FROM => ("isDisjointFrom", 1),
        // Accessor id — its name is the spec getter name and does
        // NOT intern back (excluded from the round-trip test).
        ANY_METHOD_GET_SIZE => ("get size", 0),
        ANY_METHOD_OBJECT_TO_STRING => ("toString", 0),
        // §17 built-in accessor functions carry "get " / "set "
        // prepended to the property name (the `get size` precedent
        // above); lengths follow bun — both 0 for the Annex B pair.
        ANY_METHOD_PROTO_GET => ("get __proto__", 0),
        ANY_METHOD_PROTO_SET => ("set __proto__", 0),
        // %ThrowTypeError%'s name is the empty string (§10.2.4.1).
        ANY_METHOD_THROW_TYPE_ERROR => ("", 0),
        // §20.5.3.4 — Error.prototype.toString, name "toString"
        // length 0 (same posture as the OBJECT_TO_STRING badge row).
        ANY_METHOD_ERROR_TO_STRING => ("toString", 0),
        // §23.1.3.36 — Array.prototype.toString, name "toString"
        // length 0 (same posture; RFC 20260721 刀 11 G12).
        ANY_METHOD_ARR_TO_STRING => ("toString", 0),
        // §20.4.3.3/.4 — Symbol.prototype.toString / valueOf (the
        // tag-5 thisSymbolValue cells; same posture).
        ANY_METHOD_SYMBOL_TO_STRING => ("toString", 0),
        ANY_METHOD_SYMBOL_VALUE_OF => ("valueOf", 0),
        // §20.4.3.2 — get Symbol.prototype.description (accessor id,
        // `get size` posture: spec getter name, never interns back).
        ANY_METHOD_GET_DESCRIPTION => ("get description", 0),
        // §22.1.3.36 — String.prototype[Symbol.iterator] (own id,
        // never interns back; the spec function name has brackets).
        ANY_METHOD_STR_ITERATOR => ("[Symbol.iterator]", 0),
        // Annex B §B.2.4.1 — RegExp.prototype.compile(pattern, flags).
        ANY_METHOD_COMPILE => ("compile", 2),
        // The `any_method_iter` id block (iterator protocol + weak
        // deref) rows live in [`iter_method_meta`] — the r405 watch
        // said the next mid added here must extract a family first,
        // and rotation 434's drop/toArray rows were that mid.
        _ => return iter_method_meta(mid),
    })
}

/// The `any_method_iter` id block's rows of [`any_method_meta`] —
/// extracted family (the parent's `_` arm delegates here, so a miss
/// still answers `None`).
fn iter_method_meta(mid: i64) -> Option<(&'static str, u32)> {
    use crate::any_method_iter as it;
    Some(match mid {
        // §27.1.2.1 — %Iterator.prototype%[Symbol.iterator]
        // return-this (own id, never interns back).
        m if m == it::ANY_METHOD_ITER_SELF => ("[Symbol.iterator]", 0),
        // §27.1.4.1 — %Iterator.prototype%[Symbol.dispose] (own id,
        // never interns back; RFC 20260809 B6).
        m if m == it::ANY_METHOD_ITER_DISPOSE => ("[Symbol.dispose]", 0),
        // §26.1.3.2 — WeakRef.prototype.deref. Missing until rotation
        // 383: the dispatch arm resolved it, so calls worked, but the
        // reflection faces reading this table (`.name` / `.length` /
        // the own-name enumeration) could not see it.
        m if m == it::ANY_METHOD_DEREF => ("deref", 0),
        // §27.1.4.3 / §27.1.4.10 — %Iterator.prototype% drop /
        // toArray (rotation 434: the tag-15 ownership row made the
        // reflection faces reach them; the name-table guard caught
        // the missing rows).
        m if m == it::ANY_METHOD_DROP => ("drop", 1),
        m if m == it::ANY_METHOD_TO_ARRAY => ("toArray", 0),
        _ => return None,
    })
}

/// Family-aware twin of [`any_method_meta`] — `fam` is the builtin
/// proto tag the reified cell was minted for (see
/// [`crate::builtin_proto`]). It resolves the table's single length
/// divergence: §21.1.6.6 `Number.prototype.toString(radix)` has
/// length 1 where every other prototype's `toString` has 0. Names
/// never diverge, so they pass straight through.
///
/// Callers that have no family (a family-less mint answers -1) get
/// the majority row, i.e. [`any_method_meta`]'s value.
pub fn any_method_meta_for(fam: i64, mid: i64) -> Option<(&'static str, u32)> {
    let (name, arity) = any_method_meta(mid)?;
    if mid == ANY_METHOD_TO_STRING && fam == crate::builtin_proto::NUMBER_PROTO_TAG as i64 {
        return Some((name, 1));
    }
    Some((name, arity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::any_method_intern::any_method_id;

    #[test]
    fn meta_round_trips_every_interned_name() {
        // Every id the intern table can answer must carry metadata
        // whose name interns back to the same id.
        for mid in 1..=ANY_METHOD_IS_DISJOINT_FROM {
            let (name, _) =
                any_method_meta(mid).unwrap_or_else(|| panic!("mid {mid} has no metadata row"));
            assert_eq!(any_method_id(name), mid, "name {name:?} round-trip");
        }
    }

    #[test]
    fn meta_rejects_unknown_and_out_of_table() {
        assert!(any_method_meta(ANY_METHOD_UNKNOWN).is_none());
        assert!(any_method_meta(ANY_METHOD_STR_ITERATOR + 1).is_none());
        assert!(any_method_meta(-1).is_none());
    }

    #[test]
    fn family_aware_length_resolves_the_tostring_divergence() {
        use crate::builtin_proto::{BOOLEAN_PROTO_TAG, NUMBER_PROTO_TAG, STRING_PROTO_TAG};

        // §21.1.6.6 — Number.prototype.toString takes a radix.
        assert_eq!(
            any_method_meta_for(NUMBER_PROTO_TAG as i64, ANY_METHOD_TO_STRING),
            Some(("toString", 1))
        );
        // Every other prototype's toString is 0, including the
        // family-less mint.
        for fam in [STRING_PROTO_TAG as i64, BOOLEAN_PROTO_TAG as i64, -1] {
            assert_eq!(
                any_method_meta_for(fam, ANY_METHOD_TO_STRING),
                Some(("toString", 0)),
                "fam {fam}"
            );
        }
        // No other id diverges — the family is inert for them.
        assert_eq!(
            any_method_meta_for(NUMBER_PROTO_TAG as i64, ANY_METHOD_TO_FIXED),
            any_method_meta(ANY_METHOD_TO_FIXED)
        );
        assert_eq!(
            any_method_meta_for(NUMBER_PROTO_TAG as i64, ANY_METHOD_VALUE_OF),
            any_method_meta(ANY_METHOD_VALUE_OF)
        );
        assert!(any_method_meta_for(NUMBER_PROTO_TAG as i64, ANY_METHOD_UNKNOWN).is_none());
    }

    #[test]
    fn accessor_id_has_meta_but_no_intern_row() {
        let (name, len) = any_method_meta(ANY_METHOD_GET_SIZE).unwrap();
        assert_eq!((name, len), ("get size", 0));
        assert_eq!(any_method_id(name), ANY_METHOD_UNKNOWN);
        assert_eq!(any_method_id("size"), ANY_METHOD_UNKNOWN);
    }
}
