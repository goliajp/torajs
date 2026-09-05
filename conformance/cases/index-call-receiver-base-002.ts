// The typed half of §13.3.6.2 for index callees. A fn-typed array
// element is read as a `Type::Closure` value and called through the
// env-first CallIndirect, which had no base to seed: `ops[i]()`
// answered `this === undefined` where the spec (and bun) say the
// receiver is `ops`.
//
// The seed rides the existing FLAG_CLOSURE_RECV_FIRST gate rather
// than widening the ABI, because whether the callee carries a
// `__this` slot is a runtime fact about the value in the slot. So the
// ordinary-callee arm is emitted exactly as before and `ops[i](x)`
// keeps its typed CallIndirect — `plainSum` below is the witness that
// a callee which never mentions `this` is untouched.
let kind = function () {
  return typeof (this as any);
};

const ops = [kind, kind, kind];
let seen = "";
for (let i = 0; i < 3; i++) {
  seen += ops[i]();
}
console.log(seen);

// The base is that array, not merely "an object".
let isOps = function () {
  return (this as any) === ops;
};
const probes = [isOps, isOps];
// `probes`, not `ops` — so a receiver seeded from the wrong binding
// would read false rather than accidentally passing.
console.log(probes[0](), probes[1]());

let mine = function () {
  return (this as any) === probes;
};
probes[0] = mine;
console.log(probes[0](), probes[0]());

// Arguments still land where the signature puts them.
let tagged = function (x: number, y: number) {
  return typeof (this as any) + ":" + (x + y);
};
const withArgs = [tagged, tagged];
console.log(withArgs[0](1, 2), withArgs[1](3, 4));

// A callee that never reads `this` is on the untouched path.
const plainSum = [(a: number, b: number) => a + b];
console.log(plainSum[0](2, 3), plainSum[0](4, 5));

// Detaching the read drops the base (§10.2.1.2), and must keep doing so.
const held = ops[0];
console.log(held());
