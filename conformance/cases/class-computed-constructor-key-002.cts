// Two computed "constructor" keys are two ordinary methods, so the
// §15.7.1 "more than one occurrence of constructor" early error does
// not apply — the later definition simply wins (Script goal, where
// duplicate class elements are legal).
class C {
  ["constructor"]() { return 1; }
  ["constructor"]() { return 2; }
}
console.log("two", new C().constructor());

// A real duplicate constructor is still the early error.
var threw = "no";
try {
  eval("class D { constructor() {} constructor() {} }");
} catch (e) {
  threw = e.constructor.name;
}
console.log("dup", threw);
