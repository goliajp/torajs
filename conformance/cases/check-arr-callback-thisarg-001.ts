// P1 wedge — Array.prototype callback methods accept an optional
// trailing thisArg per ES spec §23.1.3.X (map / filter / every /
// some / forEach / find / findIndex / findLast / findLastIndex /
// flatMap). Pre-fix tora's call-site arity check rejected the 2-
// arg form `xs.map(cb, thisArg)` with 'expected 1 argument(s),
// got 2'. Test262 uses these pervasively for receiver-override
// patterns — 70+ cases unblocked across the broader sample under
// built-ins/Array/prototype/{map,filter,every,some,forEach,find*,
// flatMap}/*.
//
// Implementation: check.rs Call typecheck — before the strict
// arity check, detect the pattern `Member.METHOD(cb, thisArg)`
// where METHOD is one of the listed callback methods AND args
// has exactly params.len()+1 elements. Type-check the trailing
// arg (so its internal errors still surface) then pop it.
//
// Substrate trade-off: tora's callbacks don't have `this`
// semantics (closures don't bind a receiver), so the thisArg is
// silently dropped — tests that don't rely on `this` inside the
// callback now typecheck. Tests that DO use `this` were already
// blocked on the missing-this substrate; the silent drop doesn't
// make those worse. Documented in `feedback_no_tech_debt`'s
// allowed exceptions as a "wedge" — the gate is "1 substrate
// path narrowed, no extra failure modes introduced".

let xs = [1, 2, 3]

// map with thisArg (dropped) — receiver-override pattern.
let doubled = xs.map((x) => x * 2, null)
console.log(doubled[0])                       // 2
console.log(doubled[2])                       // 6

// filter with thisArg.
let positives = xs.filter((x) => x > 0, null)
console.log(positives.length)                 // 3

// every with thisArg.
console.log(xs.every((x) => x > 0, null))    // true
console.log(xs.every((x) => x > 1, null))    // false

// some with thisArg.
console.log(xs.some((x) => x > 2, null))     // true
console.log(xs.some((x) => x > 10, null))    // false

// forEach with thisArg — body still runs (just print to verify
// callback invocation; capturing-mutate substrate is separate).
xs.forEach((x) => { console.log(x); }, null)
// 1
// 2
// 3

// find with thisArg — find returns Boolean per existing tora
// sig (the v0 narrow); cast to Number when needed.
console.log(xs.findIndex((x) => x === 2, null))  // 1

// Regression: 1-arg form still works.
let tripled = xs.map((x) => x * 3)
console.log(tripled[1])                       // 6
