// S210 — `s.search()` / `s.search(undefined)` returns 0 per ES
// §22.1.3.20 step 3: RegExpCreate(undefined, undefined) yields
// an empty regex, which matches at index 0 of any string. Pre-
// fix tora declared `(String) -> Number` so the 0-arg call
// failed at the arity gate and the 1-arg-undefined call failed
// with "argument 0: expected String, got Undefined".

console.log("hello".search())
console.log("hello".search(undefined))
console.log("".search())
console.log("".search(undefined))

// Confirm existing 1-arg-string path unchanged: explicit needle
// still routes through indexOf semantics.
console.log("hello".search("ll"))
console.log("hello".search("zz"))
