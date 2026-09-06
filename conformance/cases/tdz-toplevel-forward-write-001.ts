// §6.2.5.6 — PutValue on a top-level `let` still in its dead zone.
// The same Reference as a read, so the same mark carries it, and the
// assign lane raises where the ident lane does. It has to be asked
// BEFORE the unresolvable-Reference arm: both marks can be present at
// once (the checker files a write it cannot resolve, and a dead-zone
// binding with a non-literal initializer is exactly such a write),
// and "declared but uninitialized" is the truer of the two.
//
// A `const` target is deliberately not here: `a = 1` for a `const a`
// is an early error (§13.15.1), which is a different rule.
try {
  count = 1;
} catch (e) {
  console.log((e as Error).message);
}
let count = 5;
console.log(count);

try {
  label = "z";
} catch (e) {
  console.log((e as Error).message);
}
let label = "q";
console.log(label);

// Non-literal initializer — the shape that carries both marks.
try {
  shape = { v: 2 };
} catch (e) {
  console.log((e as Error).message);
}
let shape = { v: 1 };
console.log(shape.v);

// An ordinary write to an already-declared binding still writes.
let total = 1;
total = 9;
console.log(total);

// A target that really is not declared keeps its own message.
try {
  (globalThis as any).nope;
  missing = 3;
} catch (e) {
  console.log((e as Error).message);
}
console.log("after");
