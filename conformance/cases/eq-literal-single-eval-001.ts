// RFC 20260705 chunk 559 — decline-after-lower in the str-eq literal
// fast path: `step() === "foo"` with a non-Str-typed step() lowered
// the call inside try_inline_str_eq_with_literal, declined on the
// type test, and the generic binop path lowered it AGAIN — step()
// evaluated twice (tr count=2 vs bun 1). The lowered operand now
// parks in the chunk-555 take-once hint for the redispatch to reuse.
let count = 0;
function step(): any {
  count = count + 1;
  return "foo";
}
let eq = step() === "foo";
console.log(eq);
console.log(count);

// !== flip through the same lane.
let ne = step() !== "bar";
console.log(ne);
console.log(count);

// literal on the left picks the same fast path with other=right.
let eqL = "foo" === step();
console.log(eqL);
console.log(count);

// non-matching value still single-eval.
function other(): any {
  count = count + 1;
  return 42;
}
let eq2 = other() === "foo";
console.log(eq2);
console.log(count);

// typed-Str receiver keeps the inline fast path (no decline).
let s = "fo" + "o";
console.log(s === "foo");
console.log(s !== "foo");
