// §22.2.6.11 step 14.j calls a replacer function with
// `Call(replaceValue, undefined, «matched, captures…, position,
// string»)`. A JS function that declares fewer parameters than that
// simply never sees the rest — which is how t262 writes a replacer
// that only needs a constant, or only needs the match.
//
// tr counted the other way round: it read the callback's declared
// arity and demanded the regex have exactly that many capture groups,
// so `function () { return "X" }` and `function (m) { … }` over a
// two-group pattern both refused to compile. The argument count is the
// REGEX's to decide; the callback only has to declare no MORE than it
// will be handed.
//
// The `(number, string)` tail is the one shape that still has to match
// exactly: naming the offset and input pins every position before
// them, so a callback that named them while declaring fewer captures
// would read a capture string as the number.

// declares nothing at all
console.log("abc".replace(/b/, function () {
  return "X";
}));
console.log("a-b-c".replaceAll(/-/g, function () {
  return "+";
}));

// declares the match, ignores both captures
console.log("a1b2".replace(/([a-z])(\d)/g, function (m: string) {
  return "[" + m + "]";
}));

// declares the match and the first capture only
console.log("a1b2".replace(/([a-z])(\d)/g, function (m: string, p1: string) {
  return p1 + "/";
}));

// the full spec-shaped arity still works, tail and all
console.log(
  "a1b2".replace(
    /([a-z])(\d)/g,
    function (m: string, p1: string, p2: string, off: number, s: string) {
      return p2 + p1 + ":" + off + "/" + s.length;
    },
  ),
);

// a group that DOES participate reads back through a shorter arity
// too (the non-participating case is a separate open hole: §22.2.6.11
// hands the callback `undefined`, and a `Str`-typed parameter slot has
// no way to carry it, so tr passes "" — measured here rather than
// asserted, since asserting it would freeze the wrong answer)
console.log("xyz".replace(/x(y)?(z)/, function (m: string, p1: string) {
  return "<" + p1 + ">";
}));

// the plain-string needle form takes the same zero-arity replacer
console.log("abc".replace("b", function () {
  return "X";
}));
