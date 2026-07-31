// RFC 20260730-undeclared-ident write position — §6.2.5.6 PutValue on
// an unresolvable Reference in strict code raises ReferenceError.
// Module code is always strict, so `x = 1` with no declaration anywhere
// throws a catchable ReferenceError instead of minting a global.
try {
  x = 1;
  console.log("no-throw");
} catch (e) {
  console.log("simple", e instanceof ReferenceError);
}

// Simple assignment: the RHS evaluates BEFORE PutValue throws
// (§13.15.2: rref evaluation precedes PutValue).
const order: string[] = [];
function side(): number {
  order.push("rhs");
  return 2;
}
try {
  y = side();
} catch (e) {
  order.push("threw");
}
console.log(order.join(","));

// Compound assignment: GetValue on the target runs FIRST (§13.15.2
// step 1-2), so the RHS side effect never happens.
const order2: string[] = [];
function side2(): number {
  order2.push("rhs2");
  return 2;
}
try {
  z *= side2();
} catch (e) {
  order2.push("threw2");
}
console.log(order2.join(","));

// Update expressions: GetValue first → ReferenceError.
try {
  w++;
} catch (e) {
  console.log("post-incr", e instanceof ReferenceError);
}
try {
  ++v;
} catch (e) {
  console.log("pre-incr", e instanceof ReferenceError);
}
try {
  u--;
} catch (e) {
  console.log("post-decr", e instanceof ReferenceError);
}

// Error message parity with bun/JSC.
try {
  q = 5;
} catch (e) {
  console.log((e as Error).message);
}
