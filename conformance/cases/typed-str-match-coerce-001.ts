// RFC 20260716-primitive-wrapper-substrate 刀 9 — typed-String
// receiver `.match(...)` / `.matchAll(...)` now accept a non-RegExp
// argument via ES §22.1.3.{11,12} step 4.c `RegExpCreate(ToString(P),
// flags)`. Extends 刀 8 which only covered the any-receiver lane.
//
// Sweep target: pass→bug residual + a broader corpus of test262
// String/prototype/match_A* cases that pass a string pattern —
// pre-fix they hit `type error: argument 0: expected RegExp, got
// String` at checker (the method table declared `[RegExp]` args).
//
// checker path: `check_type_of_member_string` (String, "match"|
// "matchAll") args → `Type::Any`; SSA path:
// `ssa_lower_call_str_regex_methods::try_lower` — a non-RegExp
// arg on match / matchAll falls into the coerce lane: coerce_to_str
// + regex_compile(pat, flags) + regex_match/regex_match_all +
// regex_drop. match uses `""` flags; matchAll uses `"g"` per spec
// step 4.c (kernel throws TypeError on non-global).

function first(m: string[] | null): string {
    if (m !== null) return m[0];
    return "null";
}

// String arg — match returns Array<Str> or null.
console.log(first("hello world".match("world")));    // "world"
console.log(first("hello".match("no")));             // "null"

// String arg with capture group.
const m2 = "abcdef".match("(bc)(de)");
if (m2 !== null) console.log(m2[1] + "|" + m2[2]);   // "bc|de"

// String arg — matchAll with "\d" pattern (fold via for-of).
let acc = "";
for (const m of "a1b2c3d4".matchAll("\\d")) acc += m[0];
console.log(acc);                                     // "1234"

// Number arg — ToString(42) → "42" pattern. Exercises the F64
// coerce_to_str path in `ssa_lower_call_str_regex_methods`.
console.log(first("test42foo".match(42)));            // "42"

// Ident-bound string pattern (locals, not literal).
const pat = "wo(rl)d";
const m3 = "hello world".match(pat);
if (m3 !== null) console.log(m3[1]);                  // "rl"

// RegExp arg (unchanged path — regression sentinel).
const m6 = "hello world".match(/wo(rl)d/);
if (m6 !== null) console.log(m6[1]);                  // "rl"

// Substr receiver — chunk-800 view materialization still fires.
const chars = "match me here";
const view = chars.slice(6, 8); // "me"
console.log(first(view.match("me")));                 // "me"

// Null / Boolean args are follow-up (`coerce_to_str` has no
// Type::Ptr / Type::Bool arm on this hot-path — see L3b).
