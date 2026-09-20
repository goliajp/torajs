// ES §21.3.2.24/25 — Math.min / Math.max run ToNumber on every
// argument, and ToNumber reaches every value. The lane used to admit
// Number | Undefined | Any by name and panic the lowerer on the rest.
console.log(Math.max({}, 1));
console.log(Math.min({}, 1));

const vo = { valueOf() { return 7; } };
console.log(Math.max(vo, 1));
console.log(Math.min(vo, 1));

console.log(Math.max(true, false));
console.log(Math.min(null, 1));
console.log(Math.max("3", 2));
console.log(Math.max([5], 2));

// Single-argument fast path takes the same road.
console.log(Math.max(vo));
console.log(Math.min("4"));

// A user valueOf runs here and may throw; the pending throw must
// unwind rather than leave a NaN placeholder behind.
const boom = { valueOf(): number { throw new Error("boom"); } };
try {
  console.log(Math.max(boom, 1));
} catch (e) {
  console.log("caught " + (e as Error).message);
}
console.log(Math.max(2, 3));
