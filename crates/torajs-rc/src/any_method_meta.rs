//! Method id → reflection metadata for `any`-receiver method cells
//! — split out of `any_method.rs` by the 500-line file discipline
//! (the id constants + intern tables stay there; this sibling holds
//! the `(name, length)` reflection row per id).

// The meta table references every ANY_METHOD_* constant — a glob
// import keeps the sibling in lockstep as ids append.
use crate::any_method::*;

/// Method id → `(canonical name, ES-spec "length")` reflection
/// metadata (chunk 715) — the `.name` / `.length` reads off a
/// reified method cell (`method_value`). `None` for
/// [`ANY_METHOD_UNKNOWN`] and out-of-table ids.
///
/// Lengths are the spec `length` property values (expected argument
/// counts, ES2024). One interned id can serve several prototypes;
/// the only length divergence in the table is `toString`
/// (Number.prototype 1 vs String/Boolean/Date/RegExp 0) — the
/// majority value 0 wins, the Number deviation is a recorded
/// boundary of the per-mid (not per-prototype) cell interning.
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
        ANY_METHOD_TO_STRING => ("toString", 0),
        ANY_METHOD_TO_FIXED => ("toFixed", 1),
        ANY_METHOD_TO_EXPONENTIAL => ("toExponential", 1),
        ANY_METHOD_TO_PRECISION => ("toPrecision", 1),
        ANY_METHOD_TEST => ("test", 1),
        ANY_METHOD_EXEC => ("exec", 1),
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
        ANY_METHOD_PROPERTY_IS_ENUMERABLE => ("propertyIsEnumerable", 1),
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
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_round_trips_every_interned_name() {
        // Every id the intern table can answer must carry metadata
        // whose name interns back to the same id.
        for mid in 1..=ANY_METHOD_SUP {
            let (name, _) =
                any_method_meta(mid).unwrap_or_else(|| panic!("mid {mid} has no metadata row"));
            assert_eq!(any_method_id(name), mid, "name {name:?} round-trip");
        }
    }

    #[test]
    fn meta_rejects_unknown_and_out_of_table() {
        assert!(any_method_meta(ANY_METHOD_UNKNOWN).is_none());
        assert!(any_method_meta(ANY_METHOD_SUP + 1).is_none());
        assert!(any_method_meta(-1).is_none());
    }
}
