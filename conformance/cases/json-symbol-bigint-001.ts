// Rotation 207 — the two value-level verdicts §25.5.2.4 reaches
// without unfolding a shape. A Symbol serializes to NOTHING, like
// `undefined` and a callable, so the three-way split applies: the
// whole call answers `undefined` at the top level, an object omits
// the key, an array writes `null`. A BigInt is step 10's TypeError.
// Both used to reach the any-lane catch-all and answer `{}`, and a
// statically typed receiver was rejected outright at lowering.

console.log("A", JSON.stringify(Symbol("s")));
const sy = Symbol("t");
console.log("B", JSON.stringify(sy));
console.log("C", JSON.stringify({ s: Symbol("s") }));
console.log("D", JSON.stringify([Symbol("s")]));
console.log("E", JSON.stringify({ a: 1, s: Symbol("x"), b: 2 }));
// (A mixed-element array holds its elements as `any`, so a Symbol in
// one rides the runtime sentinel rather than this static arm — that
// conversion is incomplete and tracked separately, along with the
// plain `undefined` faces it shares a root with.)

try {
  console.log("G no-throw", JSON.stringify(1n));
} catch (e) {
  console.log("G", e instanceof TypeError);
}
const bg = 2n;
try {
  console.log("H no-throw", JSON.stringify(bg));
} catch (e) {
  console.log("H", e instanceof TypeError);
}
try {
  console.log("I no-throw", JSON.stringify({ v: 3n }));
} catch (e) {
  console.log("I", e instanceof TypeError);
}
try {
  console.log("J no-throw", JSON.stringify([4n]));
} catch (e) {
  console.log("J", e instanceof TypeError);
}

// The walk still serves ordinary values after a caught throw.
console.log("N", JSON.stringify({ ok: 1, s: "two" }));
