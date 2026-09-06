// A read that RAISES hands its lane a stand-in operand — the raise
// already happened and everything past the throw-check is unreachable,
// so only the operand's shape is still wanted. Two lanes read that
// shape to pick a receiver and rejected the program at COMPILE time
// when they saw the stand-in's Ptr: `o.v` and `xs[0]`. bun raises at
// the read and carries on, so the whole program was lost to a
// diagnostic about a line that never runs.
//
// Both raising marks reach this: an unresolvable Reference (§6.2.5.5)
// and a dead-zone binding (§8.1.1.1).
try {
  console.log(shape.v);
} catch (e) {
  console.log((e as Error).message);
}
const shape = { v: 1 };
console.log(shape.v);

try {
  console.log(items[0]);
} catch (e) {
  console.log((e as Error).message);
}
const items = [7];
console.log(items[0]);

try {
  console.log(nested.a.b);
} catch (e) {
  console.log((e as Error).message);
}
const nested = { a: { b: 2 } };
console.log(nested.a.b);

// A receiver that really is not declared, same two lanes.
try {
  console.log(nope.v);
} catch (e) {
  console.log((e as Error).message);
}
try {
  console.log(nope[0]);
} catch (e) {
  console.log((e as Error).message);
}
console.log("after");
