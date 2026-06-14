// W-N-a — Object.getOwnPropertyNames(struct-obj) returns own keys in insertion order
// Struct-obj path works today (typed struct → static key list). 3 shapes:
// empty object, single-key, multi-key. Bun parity verified byte-equal.
// Known gaps tracked in plan-state L3b:
//   - Object.getOwnPropertyNames(Arr) — SSA-lower requires struct arg (W-N-b)
//   - Object.getOwnPropertySymbols(*) — check.rs no member (W-N-c)

console.log(Object.getOwnPropertyNames({}));
console.log(Object.getOwnPropertyNames({ x: 42 }));
console.log(Object.getOwnPropertyNames({ a: 1, b: 2, c: 3 }));
