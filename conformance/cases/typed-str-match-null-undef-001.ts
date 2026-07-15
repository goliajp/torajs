// RFC 20260716-primitive-wrapper-substrate 刀 11 — typed-Str
// `.match(null)` / `.match(undefined)` support. Extends 刀 9's
// coerce_match_lane to hand off through `emit_to_string` instead
// of `coerce_to_str` — emit_to_string has the 刀 6 `Type::Ptr`
// arm that distinguishes ConstPtrNull's undef-vs-null via
// `expr_types`, so ES §22.1.3.11 step 4.c
// `RegExpCreate(ToString(null), "")` / `RegExpCreate(ToString(undef), "")`
// resolves to `/null/` / `/undefined/` regex correctly.

function first(m: string[] | null): string {
    if (m !== null) return m[0];
    return "MISS";
}

// null arg — ToString(null) = "null" per §7.1.17 step 2, then
// `RegExpCreate("null", "")` matches the literal string "null".
console.log(first("nullish".match(null)));            // "null"
// undefined arg — spec §22.2.3.2 RegExpInitialize step 1:
// `pattern is undefined → P = ""`. Matches the empty regex
// against every position; result is [""] (empty match at index 0).
console.log(first("say undefined here".match(undefined))); // ""

// matchAll — the "g" flag applies to the coerced RegExp.
let acc = "";
for (const m of "n=null n=null".matchAll(null)) acc += m[0] + "|";
console.log(acc);                                       // "null|null|"

// Miss path — string doesn't contain the literal "null".
console.log(first("no here".match(null)));             // "MISS"

// Regression sentinel — string / RegExp / number args from 刀 9
// keep working.
console.log(first("hello".match("hello")));            // "hello"
console.log(first("test42foo".match(42)));             // "42"
const m = "hello".match(/hel/);
if (m !== null) console.log(m[0]);                     // "hel"
