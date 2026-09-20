// ES §21.3.2.* step 1 on every Math method that declares Number params
// is ToNumber, which reaches every value. The lane admitted Number,
// then Any, and left the rest on a lowering sink that panics.
const vo = { valueOf() { return -7; } };
const empty = {};

console.log(Math.abs(vo));
console.log(Math.abs(empty));
console.log(Math.floor(vo), Math.ceil(vo), Math.round(vo), Math.trunc(vo));
console.log(Math.sign(vo), Math.sqrt(empty));
console.log(Math.abs(true), Math.abs(false), Math.abs(null));
console.log(Math.abs("-5"), Math.floor("2.7"));
console.log(Math.abs([]), Math.abs([3]));

// Binary: pow / atan2 / imul take the same road on both slots.
const two = { valueOf() { return 2; } };
console.log(Math.pow(two, 3), Math.pow(2, two));
console.log(Math.atan2(0, two));
console.log(Math.pow("2", "3"));

// A user valueOf runs here and may throw.
const boom = { valueOf(): number { throw new Error("boom"); } };
try { console.log(Math.abs(boom)); } catch (e) { console.log("caught " + (e as Error).message); }
try { console.log(Math.pow(boom, 2)); } catch (e) { console.log("caught " + (e as Error).message); }

// The typed fast path is untouched.
console.log(Math.abs(-3), Math.pow(2, 10));
