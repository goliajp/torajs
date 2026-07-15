// RFC 20260716-primitive-wrapper-substrate 刀 8 — String.prototype
// match / search / matchAll on an **any-typed** receiver auto-coerce
// a non-RegExp argument per ES §22.1.3.{11,12,13}. Sweep-flagged
// pass→bug residual: 6 of the 9 case cluster in handoff 112 were
// `String/prototype/search|replace/S15.5.4.11_A1_T12 + A{1_T9,
// 2_T{1,2,6,7}}` — all fired the "any receiver requires a RegExp
// argument" TypeError against a string pattern which bun quietly
// coerces via `RegExpCreate`.
//
// Runtime path: `coerce_regexp` shim in method_call_str.rs — a
// RegExp-cell arg passes through borrowed (caller-owned); a
// primitive/wrapper arg calls `__torajs_regex_compile(ToString(arg),
// flags)` and the resulting cell drops post-kernel. match/search
// pass empty flags; matchAll passes "g" per spec step 4.c.
//
// Typed-Str receiver lane (checker `expected RegExp, got String`)
// is a separate follow-up — this fixture pins only the any-lane
// via StringWrapper receivers (刀 3 view-through) and any-typed
// helper indirection.

// StringWrapper receiver — the recv goes through `str_method` any-
// lane, and 刀 8 coerces the non-RegExp arg.
console.log(new String("hello world").search("world"));  // 6
console.log(new String("hello world").search(/world/));  // 6
console.log(new String("hello world").search(/nope/));   // -1
console.log(new String("hello world").search("nope"));   // -1

// match — string arg auto-coerces to /world/ .
const m1 = new String("hello world").match("world");
console.log(m1 !== null ? m1[0] : null);      // "world"
const m2 = new String("hello world").match(/wo(rl)d/);
console.log(m2 !== null ? m2[1] : null);      // "rl"
console.log(new String("hello world").match("nope"));    // null

// matchAll — string arg auto-gets "g" flag; iterator yields all.
const iter = new String("a1b2c3").matchAll("\\d");
let acc = "";
for (const m of iter) acc += m[0];
console.log(acc);                              // "123"

// Wrapper-string arg on wrapper receiver.
console.log(new String("hello world").search(new String("wo"))); // 4

// Number arg on wrapper receiver — ToString(42) → "42".
console.log(new String("test42foo").search(42));  // 4
