// S133-1 — Map/Set/Array.forEach accept callbacks with fewer args than
// the formal sig declares (JS spec: callback arity is upper-bounded
// by the formal sig, not exact). Pre-S133 tora required strict arity
// equality on Type::Function → `m.forEach((v) => ...)` rejected with
// "expected Function([Any, Any, Map], Void), got Function([Any], Void)".
// Fix narrow: check.rs Call typecheck gains a Function-vs-Function
// subtype path — actual arity ≤ formal arity, every prefix slot
// matches (or either side is Any), return types match (or either
// side is Any).
//
// Fixture uses primitive-typed accumulators (Number, captured via
// closure `+=`) which work; Map/Set dispatch is observed via direct
// console.log inside the callback rather than closure-captured
// string/array mutating methods (those hit an orthogonal closure-
// capture wedge — recorded as L3b backlog).

// 1. Array.forEach 1-arg callback — baseline (was already legal).
let arr = [10, 20, 30];
arr.forEach((v) => console.log("arr-1arg", v));

// 2. Map.forEach 1-arg callback (value only).
let m = new Map<string, number>();
m.set("a", 1);
m.set("b", 2);
m.set("c", 3);
m.forEach((v) => console.log("map-1arg", v));

// 3. Map.forEach 2-arg callback (value, key).
m.forEach((v, k) => console.log("map-2arg", k, v));

// 4. Map.forEach full 3-arg callback (spec sig) — regression guard.
let m2 = new Map<string, number>();
m2.set("p", 100);
m2.forEach((v, k, map) => console.log("map-3arg", k, v));

// 5. Set.forEach 1-arg callback (value only).
let s = new Set<number>();
s.add(7);
s.add(11);
s.add(13);
s.forEach((v) => console.log("set-1arg", v));

// 6. Set.forEach full 2-arg callback (spec sig) — regression guard.
let s2 = new Set<number>();
s2.add(5);
s2.forEach((v, v2) => console.log("set-2arg", v, v2));

// 7. Captured-primitive accumulator inside callback still works
// (number `+=` from closure scope) — covers the actual user idiom
// `m.forEach(v => sum += v)`.
let m3 = new Map<string, number>();
m3.set("x", 4);
m3.set("y", 6);
let sum = 0;
m3.forEach((v) => { sum += v as number });
console.log("map-sum", sum);
