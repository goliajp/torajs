//! Method-id constants for `any`-receiver method calls
//! (Any-method-call RFC 20260704).
//!
//! ssa-lower interns the compile-time-known method NAME into one of
//! these ids at the call site, so the runtime dispatcher
//! (`__torajs_any_method_call` → per-tag `__torajs_{arr,str}_any_*`)
//! switches on an integer instead of comparing strings — the name
//! bytes still travel alongside for TypeError messages only.
//!
//! Shared at the torajs-rc layer (the one crate every runtime tier
//! and the compiler both depend on), the same placement as `Tag` /
//! `ARR_KIND_*`. Ids are ABI between the baked compiler and the
//! staticlibs — append-only, never renumber.

/// Name not in the intern table — the runtime dispatcher answers a
/// catchable TypeError for every receiver.
pub const ANY_METHOD_UNKNOWN: i64 = 0;
/// `Array.prototype.push`.
pub const ANY_METHOD_PUSH: i64 = 1;
/// `Array.prototype.pop`.
pub const ANY_METHOD_POP: i64 = 2;
/// `String.prototype.charAt`.
pub const ANY_METHOD_CHAR_AT: i64 = 3;
/// `String.prototype.toUpperCase`.
pub const ANY_METHOD_TO_UPPER_CASE: i64 = 4;
/// `String.prototype.toLowerCase`.
pub const ANY_METHOD_TO_LOWER_CASE: i64 = 5;
/// `String.prototype.indexOf` / `Array.prototype.indexOf`.
pub const ANY_METHOD_INDEX_OF: i64 = 6;
/// `String.prototype.includes` / `Array.prototype.includes`.
pub const ANY_METHOD_INCLUDES: i64 = 7;
/// `String.prototype.slice` (`Array.prototype.slice` is a C4+ arm).
pub const ANY_METHOD_SLICE: i64 = 8;
/// `String.prototype.split`.
pub const ANY_METHOD_SPLIT: i64 = 9;
/// `String.prototype.trim`.
pub const ANY_METHOD_TRIM: i64 = 10;
/// `String.prototype.trimStart`.
pub const ANY_METHOD_TRIM_START: i64 = 11;
/// `String.prototype.trimEnd`.
pub const ANY_METHOD_TRIM_END: i64 = 12;
/// `Array.prototype.shift`.
pub const ANY_METHOD_SHIFT: i64 = 13;
/// `Array.prototype.unshift`.
pub const ANY_METHOD_UNSHIFT: i64 = 14;
/// `Array.prototype.join`.
pub const ANY_METHOD_JOIN: i64 = 15;
/// `Array.prototype.map`.
pub const ANY_METHOD_MAP: i64 = 16;
/// `Array.prototype.filter`.
pub const ANY_METHOD_FILTER: i64 = 17;
/// `Array.prototype.forEach`.
pub const ANY_METHOD_FOR_EACH: i64 = 18;
/// `Map.prototype.get`.
pub const ANY_METHOD_GET: i64 = 19;
/// `Map.prototype.set`.
pub const ANY_METHOD_SET: i64 = 20;
/// `Map.prototype.has` / `Set.prototype.has`.
pub const ANY_METHOD_HAS: i64 = 21;
/// `Map.prototype.delete` / `Set.prototype.delete`.
pub const ANY_METHOD_DELETE: i64 = 22;
/// `Set.prototype.add`.
pub const ANY_METHOD_ADD: i64 = 23;
/// `Map.prototype.clear` / `Set.prototype.clear`.
pub const ANY_METHOD_CLEAR: i64 = 24;
/// `Date.prototype.getTime`.
pub const ANY_METHOD_GET_TIME: i64 = 25;
/// `valueOf` (the Date arm aliases getTime; other receivers are a
/// C4+ boundary).
pub const ANY_METHOD_VALUE_OF: i64 = 26;
/// `Date.prototype.toISOString`.
pub const ANY_METHOD_TO_ISO_STRING: i64 = 27;
/// `toJSON` (the Date arm aliases toISOString).
pub const ANY_METHOD_TO_JSON: i64 = 28;
/// `Date.prototype.getFullYear`.
pub const ANY_METHOD_GET_FULL_YEAR: i64 = 29;
/// `Date.prototype.getUTCFullYear`.
pub const ANY_METHOD_GET_UTC_FULL_YEAR: i64 = 30;
/// `Date.prototype.getMonth`.
pub const ANY_METHOD_GET_MONTH: i64 = 31;
/// `Date.prototype.getUTCMonth`.
pub const ANY_METHOD_GET_UTC_MONTH: i64 = 32;
/// `Date.prototype.getDate`.
pub const ANY_METHOD_GET_DATE: i64 = 33;
/// `Date.prototype.getUTCDate`.
pub const ANY_METHOD_GET_UTC_DATE: i64 = 34;
/// `Date.prototype.getHours`.
pub const ANY_METHOD_GET_HOURS: i64 = 35;
/// `Date.prototype.getUTCHours`.
pub const ANY_METHOD_GET_UTC_HOURS: i64 = 36;
/// `Date.prototype.getMinutes`.
pub const ANY_METHOD_GET_MINUTES: i64 = 37;
/// `Date.prototype.getUTCMinutes`.
pub const ANY_METHOD_GET_UTC_MINUTES: i64 = 38;
/// `Date.prototype.getSeconds`.
pub const ANY_METHOD_GET_SECONDS: i64 = 39;
/// `Date.prototype.getUTCSeconds`.
pub const ANY_METHOD_GET_UTC_SECONDS: i64 = 40;
/// `Date.prototype.getMilliseconds`.
pub const ANY_METHOD_GET_MILLISECONDS: i64 = 41;
/// `Date.prototype.getUTCMilliseconds`.
pub const ANY_METHOD_GET_UTC_MILLISECONDS: i64 = 42;
/// `Date.prototype.getDay`.
pub const ANY_METHOD_GET_DAY: i64 = 43;
/// `Date.prototype.getUTCDay`.
pub const ANY_METHOD_GET_UTC_DAY: i64 = 44;
/// `Date.prototype.getTimezoneOffset`.
pub const ANY_METHOD_GET_TIMEZONE_OFFSET: i64 = 45;
/// `Date.prototype.setTime`.
pub const ANY_METHOD_SET_TIME: i64 = 46;
/// `Date.prototype.setYear` (annexB §B.2.4.2).
pub const ANY_METHOD_SET_YEAR: i64 = 47;
/// `Date.prototype.getYear` (annexB §B.2.4.1).
pub const ANY_METHOD_GET_YEAR: i64 = 48;
/// `Date.prototype.toGMTString` (annexB alias of toUTCString).
pub const ANY_METHOD_TO_GMT_STRING: i64 = 49;
/// `Date.prototype.toUTCString`.
pub const ANY_METHOD_TO_UTC_STRING: i64 = 50;
/// `Date.prototype.toDateString`.
pub const ANY_METHOD_TO_DATE_STRING: i64 = 51;
/// `Date.prototype.toLocaleString`.
pub const ANY_METHOD_TO_LOCALE_STRING: i64 = 52;
/// `Date.prototype.toLocaleDateString`.
pub const ANY_METHOD_TO_LOCALE_DATE_STRING: i64 = 53;
/// `Date.prototype.toLocaleTimeString`.
pub const ANY_METHOD_TO_LOCALE_TIME_STRING: i64 = 54;
/// `Date.prototype.setFullYear`.
pub const ANY_METHOD_SET_FULL_YEAR: i64 = 55;
/// `Date.prototype.setMonth`.
pub const ANY_METHOD_SET_MONTH: i64 = 56;
/// `Date.prototype.setDate`.
pub const ANY_METHOD_SET_DATE: i64 = 57;
/// `Date.prototype.setHours`.
pub const ANY_METHOD_SET_HOURS: i64 = 58;
/// `Date.prototype.setMinutes`.
pub const ANY_METHOD_SET_MINUTES: i64 = 59;
/// `Date.prototype.setSeconds`.
pub const ANY_METHOD_SET_SECONDS: i64 = 60;
/// `Date.prototype.setMilliseconds`.
pub const ANY_METHOD_SET_MILLISECONDS: i64 = 61;
/// `toString` (the Number arm routes bare / radix; other receivers
/// are a C4+ boundary).
pub const ANY_METHOD_TO_STRING: i64 = 62;
/// `Number.prototype.toFixed`.
pub const ANY_METHOD_TO_FIXED: i64 = 63;
/// `Number.prototype.toExponential`.
pub const ANY_METHOD_TO_EXPONENTIAL: i64 = 64;
/// `Number.prototype.toPrecision`.
pub const ANY_METHOD_TO_PRECISION: i64 = 65;
/// `RegExp.prototype.test`.
pub const ANY_METHOD_TEST: i64 = 66;
/// `RegExp.prototype.exec`.
pub const ANY_METHOD_EXEC: i64 = 67;
/// `Map.prototype.keys` / `Set.prototype.keys`.
pub const ANY_METHOD_KEYS: i64 = 68;
/// `Map.prototype.values` / `Set.prototype.values` (= keys for Set).
pub const ANY_METHOD_VALUES: i64 = 69;
/// `Map.prototype.entries` / `Set.prototype.entries` (`[k, k]`).
pub const ANY_METHOD_ENTRIES: i64 = 70;
/// Iterator-protocol `next()` (the `Tag::MapIter` arm answers an
/// IteratorResult `{ value, done }` dynobj).
pub const ANY_METHOD_NEXT: i64 = 71;
/// `String.prototype.match` (RFC 20260706-test262-bug-corpus RC-2)
/// — the `Tag::Str` arm routes a RegExp-cell argument through the
/// typed tier's `__torajs_str_match_regex` kernel.
pub const ANY_METHOD_MATCH: i64 = 72;
/// `String.prototype.replace` (RC-2c) — string- or RegExp-pattern
/// lane picked by the argument's cell tag.
pub const ANY_METHOD_REPLACE: i64 = 73;
/// `String.prototype.replaceAll` (RC-2c).
pub const ANY_METHOD_REPLACE_ALL: i64 = 74;
/// `String.prototype.startsWith` (chunk 692) — the `Tag::Str` arm
/// wraps the typed tier's encoding-aware prefix kernel.
pub const ANY_METHOD_STARTS_WITH: i64 = 75;
/// `String.prototype.endsWith` (chunk 692).
pub const ANY_METHOD_ENDS_WITH: i64 = 76;
/// `Function.prototype.call` (chunk 710) — the `Tag::Closure` arm
/// drops the thisArg (a torajs closure body cannot reference `this`)
/// and invokes the boxed dual entry with the remaining arguments.
pub const ANY_METHOD_CALL: i64 = 77;
/// `Function.prototype.apply` (chunk 710) — same thisArg drop; the
/// argument list unpacks from the second argument's Arr cell.
pub const ANY_METHOD_APPLY: i64 = 78;
/// `Function.prototype.bind` (chunk 714) — mints a bound-function
/// cell carrying the target, the bound this and the partial
/// arguments.
pub const ANY_METHOD_BIND: i64 = 79;

/// RegExp property-read ids (Any-method-call RFC 20260704 C4-3c-2)
/// — `r.source` / `r.lastIndex` / flag booleans through an `any`
/// receiver. ssa-lower interns the member NAME into one of these at
/// the read site; `__torajs_any_regexp_prop` switches on the id
/// (same append-only ABI contract as the method ids above).
pub const ANY_RPROP_SOURCE: i64 = 0;
pub const ANY_RPROP_FLAGS: i64 = 1;
pub const ANY_RPROP_LAST_INDEX: i64 = 2;
pub const ANY_RPROP_GLOBAL: i64 = 3;
pub const ANY_RPROP_IGNORE_CASE: i64 = 4;
pub const ANY_RPROP_MULTILINE: i64 = 5;
pub const ANY_RPROP_DOT_ALL: i64 = 6;
pub const ANY_RPROP_UNICODE: i64 = 7;
pub const ANY_RPROP_STICKY: i64 = 8;

/// Write-site member-name hint for `__torajs_any_member_set` —
/// `arr.length = n` through an `any` receiver must NOT become an
/// expando shadow (reads answer the real length field), so the
/// lowering interns `length` into this id and the runtime rejects
/// the Arr arm loudly (truncation semantics are a recorded
/// follow-up). Same append-only ABI contract as the ids above.
pub const ANY_WPROP_ARR_LENGTH: i64 = 100;

/// Compile-time member-name → RegExp-prop id intern (`None` = not a
/// RegExp accessor name; the member fallback keeps its normal
/// route).
pub fn any_regexp_prop_id(name: &str) -> Option<i64> {
    match name {
        "source" => Some(ANY_RPROP_SOURCE),
        "flags" => Some(ANY_RPROP_FLAGS),
        "lastIndex" => Some(ANY_RPROP_LAST_INDEX),
        "global" => Some(ANY_RPROP_GLOBAL),
        "ignoreCase" => Some(ANY_RPROP_IGNORE_CASE),
        "multiline" => Some(ANY_RPROP_MULTILINE),
        "dotAll" => Some(ANY_RPROP_DOT_ALL),
        "unicode" => Some(ANY_RPROP_UNICODE),
        "sticky" => Some(ANY_RPROP_STICKY),
        _ => None,
    }
}

/// Compile-time name → id intern. `None`-shaped misses map to
/// [`ANY_METHOD_UNKNOWN`] (the runtime throws with the name bytes).
pub fn any_method_id(name: &str) -> i64 {
    match name {
        "push" => ANY_METHOD_PUSH,
        "pop" => ANY_METHOD_POP,
        "charAt" => ANY_METHOD_CHAR_AT,
        "toUpperCase" => ANY_METHOD_TO_UPPER_CASE,
        "toLowerCase" => ANY_METHOD_TO_LOWER_CASE,
        "indexOf" => ANY_METHOD_INDEX_OF,
        "includes" => ANY_METHOD_INCLUDES,
        "slice" => ANY_METHOD_SLICE,
        "split" => ANY_METHOD_SPLIT,
        "trim" => ANY_METHOD_TRIM,
        "trimStart" => ANY_METHOD_TRIM_START,
        "trimEnd" => ANY_METHOD_TRIM_END,
        "shift" => ANY_METHOD_SHIFT,
        "unshift" => ANY_METHOD_UNSHIFT,
        "join" => ANY_METHOD_JOIN,
        "map" => ANY_METHOD_MAP,
        "filter" => ANY_METHOD_FILTER,
        "forEach" => ANY_METHOD_FOR_EACH,
        "get" => ANY_METHOD_GET,
        "set" => ANY_METHOD_SET,
        "has" => ANY_METHOD_HAS,
        "delete" => ANY_METHOD_DELETE,
        "add" => ANY_METHOD_ADD,
        "clear" => ANY_METHOD_CLEAR,
        "getTime" => ANY_METHOD_GET_TIME,
        "valueOf" => ANY_METHOD_VALUE_OF,
        "toISOString" => ANY_METHOD_TO_ISO_STRING,
        "toJSON" => ANY_METHOD_TO_JSON,
        "getFullYear" => ANY_METHOD_GET_FULL_YEAR,
        "getUTCFullYear" => ANY_METHOD_GET_UTC_FULL_YEAR,
        "getMonth" => ANY_METHOD_GET_MONTH,
        "getUTCMonth" => ANY_METHOD_GET_UTC_MONTH,
        "getDate" => ANY_METHOD_GET_DATE,
        "getUTCDate" => ANY_METHOD_GET_UTC_DATE,
        "getHours" => ANY_METHOD_GET_HOURS,
        "getUTCHours" => ANY_METHOD_GET_UTC_HOURS,
        "getMinutes" => ANY_METHOD_GET_MINUTES,
        "getUTCMinutes" => ANY_METHOD_GET_UTC_MINUTES,
        "getSeconds" => ANY_METHOD_GET_SECONDS,
        "getUTCSeconds" => ANY_METHOD_GET_UTC_SECONDS,
        "getMilliseconds" => ANY_METHOD_GET_MILLISECONDS,
        "getUTCMilliseconds" => ANY_METHOD_GET_UTC_MILLISECONDS,
        "getDay" => ANY_METHOD_GET_DAY,
        "getUTCDay" => ANY_METHOD_GET_UTC_DAY,
        "getTimezoneOffset" => ANY_METHOD_GET_TIMEZONE_OFFSET,
        "setTime" => ANY_METHOD_SET_TIME,
        "setYear" => ANY_METHOD_SET_YEAR,
        "getYear" => ANY_METHOD_GET_YEAR,
        "toGMTString" => ANY_METHOD_TO_GMT_STRING,
        "toUTCString" => ANY_METHOD_TO_UTC_STRING,
        "toDateString" => ANY_METHOD_TO_DATE_STRING,
        "toLocaleString" => ANY_METHOD_TO_LOCALE_STRING,
        "toLocaleDateString" => ANY_METHOD_TO_LOCALE_DATE_STRING,
        "toLocaleTimeString" => ANY_METHOD_TO_LOCALE_TIME_STRING,
        "setFullYear" => ANY_METHOD_SET_FULL_YEAR,
        "setMonth" => ANY_METHOD_SET_MONTH,
        "setDate" => ANY_METHOD_SET_DATE,
        "setHours" => ANY_METHOD_SET_HOURS,
        "setMinutes" => ANY_METHOD_SET_MINUTES,
        "setSeconds" => ANY_METHOD_SET_SECONDS,
        "setMilliseconds" => ANY_METHOD_SET_MILLISECONDS,
        "toString" => ANY_METHOD_TO_STRING,
        "toFixed" => ANY_METHOD_TO_FIXED,
        "toExponential" => ANY_METHOD_TO_EXPONENTIAL,
        "toPrecision" => ANY_METHOD_TO_PRECISION,
        "match" => ANY_METHOD_MATCH,
        "replace" => ANY_METHOD_REPLACE,
        "replaceAll" => ANY_METHOD_REPLACE_ALL,
        "startsWith" => ANY_METHOD_STARTS_WITH,
        "endsWith" => ANY_METHOD_ENDS_WITH,
        "test" => ANY_METHOD_TEST,
        "exec" => ANY_METHOD_EXEC,
        "call" => ANY_METHOD_CALL,
        "apply" => ANY_METHOD_APPLY,
        "bind" => ANY_METHOD_BIND,
        "keys" => ANY_METHOD_KEYS,
        "values" => ANY_METHOD_VALUES,
        "entries" => ANY_METHOD_ENTRIES,
        "next" => ANY_METHOD_NEXT,
        _ => ANY_METHOD_UNKNOWN,
    }
}

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
        for mid in 1..=ANY_METHOD_BIND {
            let (name, _) =
                any_method_meta(mid).unwrap_or_else(|| panic!("mid {mid} has no metadata row"));
            assert_eq!(any_method_id(name), mid, "name {name:?} round-trip");
        }
    }

    #[test]
    fn meta_rejects_unknown_and_out_of_table() {
        assert!(any_method_meta(ANY_METHOD_UNKNOWN).is_none());
        assert!(any_method_meta(ANY_METHOD_BIND + 1).is_none());
        assert!(any_method_meta(-1).is_none());
    }
}
