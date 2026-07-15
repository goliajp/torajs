// RFC 20260716-primitive-wrapper-substrate 刀 18 —
// `Object.defineProperty(obj, key, desc)` ToPropertyKey coerces the
// key arg per ES §20.1.2.6 step 1 → §7.1.19 → §7.1.17 ToString.
// Closes the pass→incompat residual `test/built-ins/Object/
// defineProperty/15.2.3.6-2-40.js` surfaced in the rotation-113
// sweep (`Object.defineProperty(obj, new String("Hello"), {})` —
// pre-fix checker rejected at "key must be string, got Any").
//
// Checker sig relaxed `[Any, String, Any] -> Void` to `[Any, Any,
// Any] -> Void` (mirror of 刀 17 gOPD). SSA lower's shared
// `lower_key` helper — used by defineProperty / defineProperties /
// object-literal define / accessor-pair define — now returns
// `(Operand, owned)`. Non-`Type::Str` keys route through
// `emit_to_string` and take the owned path; every helper caller
// drops the coerced Str after the runtime borrow read.
// Interned-literal (`DefineKey::Name`) and typed-Str Expr keys
// keep the fast borrow path (no drop).

// Exact test262 15.2.3.6-2-40 shape.
const obj: any = {};
Object.defineProperty(obj, new String("Hello"), {});
console.log(obj.hasOwnProperty("Hello"));  // true

// StringWrapper with a data descriptor — value shows through.
const o2: any = {};
Object.defineProperty(o2, new String("greet"), { value: "hi", enumerable: true });
console.log(o2.greet);                       // "hi"
console.log(o2.hasOwnProperty("greet"));     // true
console.log(Object.keys(o2));                // ["greet"]

// I64 key — ToString(42) = "42".
const arr: any = {};
Object.defineProperty(arr, 42, { value: "answer", enumerable: true });
console.log(arr[42]);                        // "answer"
console.log(arr["42"]);                      // "answer"
console.log(Object.keys(arr));               // ["42"]

// Boolean key — ToString(true) = "true".
const flag: any = {};
Object.defineProperty(flag, true, { value: "yes", enumerable: true });
console.log(flag.true);                      // "yes"
console.log(Object.keys(flag));              // ["true"]

// Regression: primitive string key still works (fast borrow path).
const p: any = {};
Object.defineProperty(p, "Hello", { value: 1, enumerable: true });
console.log(p.Hello);                        // 1

// Regression: object-literal-syntax define also unchanged (goes
// through the same `lower_key` via `DefineKey::Name` fast path).
const lit: any = { fixed: "yes" };
console.log(lit.fixed);                      // "yes"

// Regression: accessor-pair define via defineProperty still works
// with a StringWrapper key.
const acc: any = {};
let hits = 0;
Object.defineProperty(acc, new String("g"), {
  get() { hits++; return "got"; },
  enumerable: true,
});
console.log(acc.g);                          // "got"
console.log(acc.g);                          // "got"
console.log(hits);                           // 2
