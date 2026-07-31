// §22.2.4.1 — RegExp(pattern, flags) called as a plain function is
// construct-equivalent to new RegExp(pattern, flags).
const r1 = RegExp("ab+c");
console.log(r1.test("abbbc"));
console.log(r1.source);

const r2 = RegExp("x(\\d+)", "g");
console.log(r2.flags);
console.log("x12 x34".replace(r2, "N"));

// variable pattern rides the runtime construct (an any-typed
// pattern hits the pre-existing new-RegExp(any) silent-death gap —
// L3b, not this rewrite)
const pat = "^he";
const r3 = RegExp(pat);
console.log(r3.test("hello"), r3.test("oh hello"));

// zero-arg form matches everything-empty
const r4 = RegExp();
console.log(r4.test(""), r4.source);

// call form product is a real RegExp cell
console.log(r1 instanceof RegExp, typeof r2);

// malformed constant pattern still raises a catchable SyntaxError
try {
  RegExp("[");
} catch (e: any) {
  console.log("bad", e instanceof SyntaxError);
}
