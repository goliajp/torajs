// P7.1 — built-in `TypeError` subclass (spec §20.5.5). Injected by
// `inject_builtin_classes` as `class TypeError extends Error {
// constructor(message: string) { super(message); this.name =
// "TypeError"; } }`, flowing through the same desugar / super-rewrite
// / instanceof-chain machinery as a user-written subclass (proven by
// check-extends-builtins-001 for the generic case).
//
// Acceptance:
//   - new TypeError("msg").message === "msg" (forwarded via super)
//   - .name === "TypeError" (subclass ctor overrides Error's "Error")
//   - instanceof TypeError AND instanceof Error (chain walk)
//   - user subclass `extends TypeError` keeps the full chain + name
//   - throw + typed catch by TypeError and by Error parent
//   - .stack / .toString format stay deferred (not asserted here)

// 1. Direct instantiation.
const e1 = new TypeError("bad type");
console.log(e1.message);             // bad type
console.log(e1.name);                // TypeError
console.log(e1 instanceof TypeError); // true
console.log(e1 instanceof Error);    // true

// 2. User subclass of the builtin.
class AppTypeError extends TypeError {
  code: number;
  constructor(msg: string, code: number) {
    super(msg);
    this.code = code;
  }
}
const e2 = new AppTypeError("coercion failed", 7);
console.log(e2.message);                 // coercion failed
console.log(e2.name);                    // TypeError (inherited)
console.log(e2.code);                    // 7
console.log(e2 instanceof AppTypeError); // true
console.log(e2 instanceof TypeError);    // true
console.log(e2 instanceof Error);        // true

// 3. Throw + typed catch by the builtin subclass.
try {
  throw new TypeError("not a function");
} catch (err: TypeError) {
  console.log(err.message);            // not a function
  console.log(err.name);              // TypeError
}

// 4. Throw + catch by the Error parent.
try {
  throw new AppTypeError("nope", 42);
} catch (err: Error) {
  console.log(err.message);            // nope
}
