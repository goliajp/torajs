// eval of a declaration-only source answers undefined.
//
// Declarations complete with *empty*, so the eval's completion value is
// undefined (§14.5.1) — and in a strict eval the bindings die with the
// eval's own environment, so nothing can observe them afterwards. This
// is how test262's type-conversion suites spell "undefined":
// `Boolean(eval("var x"))` is S9.2's way of saying Boolean(undefined).
//
// `eval("var x")` with no terminator is valid JavaScript by §12.9.1
// rule 2 (automatic semicolon insertion at end of input).

console.log(Boolean(eval("var x")));

console.log(eval("var a, b") === undefined);

console.log(typeof eval("function h() {}"));

console.log(eval("let q = 5") === undefined);

// with an explicit terminator too
console.log(eval("var t;") === undefined);

// in an operand position
console.log(eval("var u") === void 0);
