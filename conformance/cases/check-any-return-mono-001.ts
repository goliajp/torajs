// P0.1 — first step of the v4 untyped-JS surface trunk. The
// existing desugar_implicit_generics pass already rewrites a
// missing-or-`any` parameter type to a fresh TypeVar (`__T<n>`)
// so call sites can monomorphise on the concrete arg type. But
// when the return type was also `: any`, it allocated a SECOND
// fresh TypeVar (`__T<m>`) with no constraint linking it to the
// param — so call-site inference bailed with 'could not infer
// type parameter `__T2` for `f`'.
//
// The fix runs the existing static return-ann sniff (the same
// one used for omitted return annotations) BEFORE allocating
// the fresh TypeVar. For `function passthrough(x: any): any
// { return x }` the sniff sees `return x` and reports `x`'s
// declared type — which by the time we get here is `__T1` (the
// param's freshly-allocated TypeVar). Returning `__T1` makes the
// call-site mono unifier bind both at once, monomorphising the
// whole function on the concrete arg type.
//
// Net effect:
// * Named declared `function f(x: any): any { return ... }`
//   now monomorphises cleanly per call site.
// * Functions with multiple Any params + a return that's
//   convertible to one of them inherit the same path (eq /
//   not / neg style).
// * Function expressions assigned to `var f = function(...)`
//   are a separate substrate item (the SSA signature gets
//   pinned at the var-binding site, no per-call-site mono);
//   covered by a follow-up P0 item, not this fixture.
//
// Acceptance: typed-tier perf 0% regression (every existing
// fixture stayed concrete-typed, this only adds a new
// path). Test262 in-scope rate inches up because some
// `function assert(x, y) { ... }` pre-rejects now typecheck.

// Single-arg passthrough — the canonical pattern that
// motivated the fix.
function id(x: any): any { return x }
console.log(id(42))                        // 42
console.log(id("hello"))                   // hello
console.log(id(true))                      // true

// Two-arg equality — the canonical test262 assert shape.
function eq(a: any, b: any): boolean { return a === b }
console.log(eq(1, 1))                      // true
console.log(eq(1, 2))                      // false
console.log(eq("hi", "hi"))                // true
console.log(eq("hi", "ho"))                // false
console.log(eq(true, true))                // true
console.log(eq(true, false))               // false

// Unary negation on Any — mono picks number at the call site.
function neg(n: any): any { return -n }
console.log(neg(5))                        // -5
console.log(neg(-3))                       // 3

// Logical NOT — mono picks bool.
function not(b: any): boolean { return !b }
console.log(not(true))                     // false
console.log(not(false))                    // true

// Nested call site — id-of-id picks the same TypeVar both
// hops, so the outer call site monomorphises both.
console.log(id(id(99)))                    // 99
console.log(id(id("nested")))              // nested
