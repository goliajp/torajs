// The pre-gate may only be reached from a real property lookup. Where
// the spec converts a value directly — §7.1.17 ToString of a number,
// a boolean, a bigint — nothing is looked up, so a patch on that
// value's prototype must not be observable.
//
// This is a boundary rather than a feature, and it cost a pass
// regression to find: torajs reaches the method dispatcher for the
// internal ToString of a bigint, so pre-gating that family made a
// patched `BigInt.prototype.toString` fire from inside a coercion
// test262 checks explicitly.

let called: string = "";
(Boolean.prototype as any).toString = function () {
  called = called + "b";
  return "PATCHED";
};
(Number.prototype as any).toString = function () {
  called = called + "n";
  return "PATCHED";
};
(BigInt.prototype as any).toString = function () {
  called = called + "B";
  return "PATCHED";
};

// internal coercions: the patch is not on the path
console.log("String(true)", String(true), "|" + called + "|");
console.log("String(1)   ", String(1), "|" + called + "|");
console.log("template    ", `${true} ${1}`, "|" + called + "|");

// ToString(O) inside a String.prototype method reached through .call
console.log(
  "isWellFormed",
  (String.prototype as any).isWellFormed.call(true),
  (String.prototype as any).isWellFormed.call(1),
  (String.prototype as any).isWellFormed.call(1n),
  "|" + called + "|",
);

// NOT asserted here: an explicit `(1n).toString()` IS a real lookup,
// and both bun and node answer the patch. torajs answers natively,
// because taking BigInt off the pre-gate takes its explicit calls off
// it too — the native arm replies before the tail consult is reached.
// Untangling those two needs the internal coercion to stop going
// through the method dispatcher; recorded rather than asserted.
