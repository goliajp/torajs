// P7.3 — `Error.prototype.stack` (minimal, header-only). `.stack`
// is non-standard ECMAScript (not in ECMA-262 / test262), but every
// engine provides it and real code reads it. Pre-fix tora's
// injected Error was `Struct([message, name])` with no `stack`, so
// `e.stack` was a typecheck error.
//
// Fix: inject_builtin_classes adds a `stack: string` field set in
// the ctor to `this.name + ": " + this.message` — the Node
// `Error.stackTraceLimit = 0` shape (a real-engine-legal value:
// header line, no frames). Synthesizing fake `at file:line` frames
// would be silent-wrong; real frame capture is a separate
// perf-sensitive substrate (P7.3-frames, deferred).
//
// Verification only asserts properties true for BOTH bun (header +
// real frames) and tora (header only): typeof, non-empty, and the
// `<Name>: <message>` prefix via startsWith. The raw stack is never
// printed (bun appends frames with absolute paths — unmatchable and
// not spec-defined).

// 1. Base Error: stack is a string, non-empty, header-prefixed.
const e = new Error("boom");
console.log(typeof e.stack);                    // string
console.log(e.stack.length > 0);                // true
console.log(e.stack.startsWith("Error: boom")); // true

// 2. Each builtin subclass: header reflects the subclass name
//    (set after super() overrides it, not "Error:").
const te = new TypeError("bad type");
console.log(te.stack.startsWith("TypeError: bad type")); // true
console.log(te instanceof Error);                        // true
const re = new RangeError("oob");
console.log(re.stack.startsWith("RangeError: oob"));     // true
const se = new SyntaxError("tok");
console.log(se.stack.startsWith("SyntaxError: tok"));    // true
const fe = new ReferenceError("undef");
console.log(fe.stack.startsWith("ReferenceError: undef")); // true

// 3. User subclass of Error: inherits stack; ctor that only calls
//    super() leaves name "Error", so the header is "Error: ...".
class MyError extends Error {
  constructor(m: string) {
    super(m);
  }
}
const u = new MyError("oops");
console.log(typeof u.stack);                  // string
console.log(u.stack.startsWith("Error: oops")); // true

// 4. User subclass that overrides name keeps stack consistent with
//    the value at construction (Error ctor builds "Error: ...";
//    subclasses re-derive after their own name set — a user class
//    that sets name in its ctor body after super() reproduces the
//    pre-override header, matching engine capture-at-super order).
class AppError extends Error {
  constructor(m: string) {
    super(m);
  }
}
const a = new AppError("db");
console.log(a.stack.startsWith("Error: db")); // true

// 5. Empty message: ECMAScript §20.5.3.4 — the header is just the
//    name, NO ": " separator (`new Error("").stack` is "Error", not
//    "Error: "). bun / V8 / JSC all do this.
const e2 = new Error("");
console.log(e2.stack.startsWith("Error"));     // true
console.log(e2.stack.startsWith("Error: "));   // false (no separator)
const t2 = new TypeError("");
console.log(t2.stack.startsWith("TypeError"));    // true
console.log(t2.stack.startsWith("TypeError: ")); // false

// 6. Special-char message keeps the ": " separator (non-empty).
const e3 = new TypeError("a: b\tc");
console.log(e3.stack.startsWith("TypeError: a: b\tc")); // true

// 7. Thrown + caught: the caught value still has its stack.
try {
  throw new RangeError("caught");
} catch (err: RangeError) {
  console.log(err.stack.startsWith("RangeError: caught")); // true
}
