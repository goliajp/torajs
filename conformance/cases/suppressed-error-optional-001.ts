// RFC 20260809 B6 — §20.5.8 SuppressedError treats all three ctor
// params as optional plain arguments: zero-arg / one-arg / two-arg
// construction defines own `error` / `suppressed` (undefined where
// absent), and `message` stays prototype-inherited "" when missing.
const e0 = new SuppressedError();
console.log(e0.error, e0.suppressed, e0.message);
console.log(e0 instanceof SuppressedError, e0 instanceof Error, e0.name);
const e1 = new SuppressedError("a");
console.log(e1.error, e1.suppressed);
const e2 = new SuppressedError("a", "b");
console.log(e2.error, e2.suppressed, e2.message);
const e3 = new SuppressedError(1, 2, "m");
console.log(e3.error, e3.suppressed, e3.message);
