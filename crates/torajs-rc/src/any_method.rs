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
/// `Object.prototype.hasOwnProperty` (RFC 20260711 chunk D-1) —
/// universal own-property probe through `__torajs_any_prop_has`.
pub const ANY_METHOD_HAS_OWN_PROPERTY: i64 = 80;
/// `Object.prototype.propertyIsEnumerable` (chunk D-1) — own +
/// enumerable-flag probe (`__torajs_any_prop_enumerable`).
pub const ANY_METHOD_PROPERTY_IS_ENUMERABLE: i64 = 81;
/// `Array.prototype.lastIndexOf` (RFC 20260711 any-dispatch
/// backfill) — backwards strict-eq scan.
pub const ANY_METHOD_LAST_INDEX_OF: i64 = 82;
/// `Array.prototype.reverse` (any-dispatch backfill) — in-place
/// 8-byte-slot swap, answers the receiver for chaining.
pub const ANY_METHOD_REVERSE: i64 = 83;
/// `Array.prototype.concat` (any-dispatch backfill chunk 2) —
/// variadic; array arguments spread, others append as one element.
pub const ANY_METHOD_CONCAT: i64 = 84;
/// `Array.prototype.fill` (chunk 2) — rides `arr_fill_any`'s
/// kind-aware write; answers the receiver for chaining.
pub const ANY_METHOD_FILL: i64 = 85;
/// `Array.prototype.copyWithin` (chunk 2) — in-place move with the
/// heap-slot rc ledger; answers the receiver for chaining.
pub const ANY_METHOD_COPY_WITHIN: i64 = 86;
/// `Array.prototype.splice` (chunk 2) — remove + variadic insert;
/// answers the removed slice.
pub const ANY_METHOD_SPLICE: i64 = 87;
/// `Array.prototype.every` (chunk 3) — early-exit predicate loop.
pub const ANY_METHOD_EVERY: i64 = 88;
/// `Array.prototype.some` (chunk 3) — early-exit predicate loop.
pub const ANY_METHOD_SOME: i64 = 89;
/// `Array.prototype.find` (chunk 3) — first matching element.
pub const ANY_METHOD_FIND: i64 = 90;
/// `Array.prototype.findIndex` (chunk 3) — first matching index.
pub const ANY_METHOD_FIND_INDEX: i64 = 91;
/// `Array.prototype.reduce` (chunk 3) — 4-arg accumulator fold.
pub const ANY_METHOD_REDUCE: i64 = 92;
/// `Array.prototype.reduceRight` (chunk 3) — backwards fold.
pub const ANY_METHOD_REDUCE_RIGHT: i64 = 93;
/// `Array.prototype.sort` (chunk 4) — in-place stable merge sort;
/// boxed-comparator or the §23.1.3.30.2 ToString default.
pub const ANY_METHOD_SORT: i64 = 94;
/// `String.prototype.anchor` — annexB B.2.2 CreateHTML surface.
/// The four attributed forms sit first (95-98) and the family stays
/// contiguous (95-107) so dispatchers range-test attr-vs-plain and
/// family membership.
pub const ANY_METHOD_ANCHOR: i64 = 95;
/// `String.prototype.fontcolor` (annexB attributed form).
pub const ANY_METHOD_FONTCOLOR: i64 = 96;
/// `String.prototype.fontsize` (annexB attributed form).
pub const ANY_METHOD_FONTSIZE: i64 = 97;
/// `String.prototype.link` (annexB attributed form).
pub const ANY_METHOD_LINK: i64 = 98;
/// `String.prototype.big` (annexB plain wrap).
pub const ANY_METHOD_BIG: i64 = 99;
/// `String.prototype.blink` (annexB plain wrap).
pub const ANY_METHOD_BLINK: i64 = 100;
/// `String.prototype.bold` (annexB plain wrap).
pub const ANY_METHOD_BOLD: i64 = 101;
/// `String.prototype.fixed` (annexB plain wrap).
pub const ANY_METHOD_FIXED: i64 = 102;
/// `String.prototype.italics` (annexB plain wrap).
pub const ANY_METHOD_ITALICS: i64 = 103;
/// `String.prototype.small` (annexB plain wrap).
pub const ANY_METHOD_SMALL: i64 = 104;
/// `String.prototype.strike` (annexB plain wrap).
pub const ANY_METHOD_STRIKE: i64 = 105;
/// `String.prototype.sub` (annexB plain wrap).
pub const ANY_METHOD_SUB: i64 = 106;
/// `String.prototype.sup` (annexB plain wrap).
pub const ANY_METHOD_SUP: i64 = 107;
/// `Date.prototype.setUTCFullYear`.
pub const ANY_METHOD_SET_UTC_FULL_YEAR: i64 = 108;
/// `Date.prototype.setUTCMonth`.
pub const ANY_METHOD_SET_UTC_MONTH: i64 = 109;
/// `Date.prototype.setUTCDate`.
pub const ANY_METHOD_SET_UTC_DATE: i64 = 110;
/// `Date.prototype.setUTCHours`.
pub const ANY_METHOD_SET_UTC_HOURS: i64 = 111;
/// `Date.prototype.setUTCMinutes`.
pub const ANY_METHOD_SET_UTC_MINUTES: i64 = 112;
/// `Date.prototype.setUTCSeconds`.
pub const ANY_METHOD_SET_UTC_SECONDS: i64 = 113;
/// `Date.prototype.setUTCMilliseconds`.
pub const ANY_METHOD_SET_UTC_MILLISECONDS: i64 = 114;
/// `String.prototype.substring`.
pub const ANY_METHOD_SUBSTRING: i64 = 115;
/// `String.prototype.substr` (annexB §B.2.2.1).
pub const ANY_METHOD_SUBSTR: i64 = 116;
/// `String.prototype.at` (ES2022 relative indexing).
pub const ANY_METHOD_AT: i64 = 117;
/// `String.prototype.charCodeAt`.
pub const ANY_METHOD_CHAR_CODE_AT: i64 = 118;
/// `String.prototype.padStart`.
pub const ANY_METHOD_PAD_START: i64 = 119;
/// `String.prototype.padEnd`.
pub const ANY_METHOD_PAD_END: i64 = 120;
/// `String.prototype.repeat`.
pub const ANY_METHOD_REPEAT: i64 = 121;
/// `String.prototype.codePointAt`.
pub const ANY_METHOD_CODE_POINT_AT: i64 = 122;
/// `String.prototype.localeCompare`.
pub const ANY_METHOD_LOCALE_COMPARE: i64 = 123;
/// `String.prototype.normalize`.
pub const ANY_METHOD_NORMALIZE: i64 = 124;
/// `String.prototype.toLocaleLowerCase`.
pub const ANY_METHOD_TO_LOCALE_LOWER_CASE: i64 = 125;
/// `String.prototype.toLocaleUpperCase`.
pub const ANY_METHOD_TO_LOCALE_UPPER_CASE: i64 = 126;
/// `String.prototype.search`.
pub const ANY_METHOD_SEARCH: i64 = 127;
/// `String.prototype.matchAll`.
pub const ANY_METHOD_MATCH_ALL: i64 = 128;
/// `String.prototype.isWellFormed` (ES2024).
pub const ANY_METHOD_IS_WELL_FORMED: i64 = 129;
/// `String.prototype.toWellFormed` (ES2024).
pub const ANY_METHOD_TO_WELL_FORMED: i64 = 130;
/// `Array.prototype.flat` (ES2019).
pub const ANY_METHOD_FLAT: i64 = 131;
/// `Array.prototype.flatMap` (ES2019).
pub const ANY_METHOD_FLAT_MAP: i64 = 132;
/// `Array.prototype.findLast` (ES2023).
pub const ANY_METHOD_FIND_LAST: i64 = 133;
/// `Array.prototype.findLastIndex` (ES2023).
pub const ANY_METHOD_FIND_LAST_INDEX: i64 = 134;
/// `Array.prototype.toReversed` (ES2023 change-array-by-copy).
pub const ANY_METHOD_TO_REVERSED: i64 = 135;
/// `Array.prototype.toSorted` (ES2023 change-array-by-copy).
pub const ANY_METHOD_TO_SORTED: i64 = 136;
/// `Array.prototype.toSpliced` (ES2023 change-array-by-copy).
pub const ANY_METHOD_TO_SPLICED: i64 = 137;
/// `Array.prototype.with` (ES2023 change-array-by-copy).
pub const ANY_METHOD_WITH: i64 = 138;
/// `Set.prototype.union` (ES2025 set methods).
pub const ANY_METHOD_UNION: i64 = 139;
/// `Set.prototype.intersection` (ES2025 set methods).
pub const ANY_METHOD_INTERSECTION: i64 = 140;
/// `Set.prototype.difference` (ES2025 set methods).
pub const ANY_METHOD_DIFFERENCE: i64 = 141;
/// `Set.prototype.symmetricDifference` (ES2025 set methods).
pub const ANY_METHOD_SYMMETRIC_DIFFERENCE: i64 = 142;
/// `Set.prototype.isSubsetOf` (ES2025 set methods).
pub const ANY_METHOD_IS_SUBSET_OF: i64 = 143;
/// `Set.prototype.isSupersetOf` (ES2025 set methods).
pub const ANY_METHOD_IS_SUPERSET_OF: i64 = 144;
/// `Set.prototype.isDisjointFrom` (ES2025 set methods).
pub const ANY_METHOD_IS_DISJOINT_FROM: i64 = 145;
/// `get Map.prototype.size` / `get Set.prototype.size` — an
/// ACCESSOR id, not a method name: it is carried by the reified
/// getter cells (`.name` answers "get size" via the meta row) and
/// deliberately absent from the intern table, so a `size` member
/// read never resolves to a method cell (the property read path
/// answers the value directly).
pub const ANY_METHOD_GET_SIZE: i64 = 146;
/// `Object.prototype.toString` (§20.1.3.6) — a DISTINCT function
/// object from every per-receiver `toString`: it classifies the
/// this-value into the "[object X]" badge instead of stringifying
/// it. Deliberately absent from the intern table (a plain
/// `toString` member read keeps resolving to the receiver's own
/// surface); handed out only by the `Object.prototype` proto-
/// singleton alias so `Object.prototype.toString` reads / `.call`
/// re-dispatch carry the badge semantics with any receiver.
pub const ANY_METHOD_OBJECT_TO_STRING: i64 = 147;
/// Annex B §B.2.2.2-5 legacy accessor surface (RFC
/// 20260713-annexb-legacy-accessor) — `Object.prototype.
/// __defineGetter__` / `__defineSetter__` install one accessor face
/// via the define kernel; `__lookupGetter__` / `__lookupSetter__`
/// answer the matching face's closure through the `__proto__` walk.
pub const ANY_METHOD_DEFINE_GETTER: i64 = 148;
/// See [`ANY_METHOD_DEFINE_GETTER`].
pub const ANY_METHOD_DEFINE_SETTER: i64 = 149;
/// See [`ANY_METHOD_DEFINE_GETTER`].
pub const ANY_METHOD_LOOKUP_GETTER: i64 = 150;
/// See [`ANY_METHOD_DEFINE_GETTER`].
pub const ANY_METHOD_LOOKUP_SETTER: i64 = 151;
/// `Object.prototype.isPrototypeOf` (§20.1.3.3) — walks the
/// argument's [[Prototype]] chain comparing identity with the
/// receiver (RFC 20260717-user-proto-chain knife 4). Universal like
/// the own-property probes.
pub const ANY_METHOD_IS_PROTOTYPE_OF: i64 = 152;
/// Annex B §B.2.2.1 `get __proto__` — the reified getter face of
/// the `Object.prototype.__proto__` accessor (RFC
/// 20260718-accessor-reify 刀 1). Deliberately absent from the
/// intern table (the name carries a space, no member read ever
/// resolves to it); handed out only through the accessor pair the
/// proto-singleton install defines.
pub const ANY_METHOD_PROTO_GET: i64 = 153;
/// Annex B §B.2.2.1 `set __proto__` — see [`ANY_METHOD_PROTO_GET`].
pub const ANY_METHOD_PROTO_SET: i64 = 154;

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
