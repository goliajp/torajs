// Reading a `number[]` out of range answers `undefined` (ES §10.4.2.1
// — an absent index has no property, so [[Get]] answers undefined),
// the way a `string[]` already did.
//
// tr stores an all-integral `number[]` in I64 slots, and an I64 slot has
// no bit pattern to spare for that answer, so the read used to raise a
// RangeError instead. Inside a named fn that was worse than loud: the
// pending throw unwound the body while the caller's throw-check was
// skipped, so the function silently stopped mid-way and the process
// still exited 0.
//
// Being read by index is now itself a reason to widen the element slot,
// the same way a fractional write is — the F64 sentinel is the only
// numeric representation of undefined there. Widening costs nothing in
// semantics: a JS number IS an f64, so the I64 form was the narrower
// one all along (the 2^53+1 case below reads identically either way).

const xs: number[] = [1, 2];

// direct, at top level
console.log(xs[5]);
console.log(typeof xs[5]);
console.log(xs[5] === undefined);
console.log(xs[-1]);

// through a variable index, in a loop past the end
for (let i = 0; i < 4; i++) console.log(i, xs[i]);

// at() takes the same exit (§23.1.3.1 step 6)
console.log(xs.at(5));
console.log(xs.at(0));

// inside a named fn: the read must not abort the body
function readPast(): void {
  const ys: number[] = [7];
  console.log("before");
  console.log(ys[9]);
  console.log("after");
}
readPast();

// in-range reads are untouched, and the values that could be confused
// with the sentinel stay themselves
console.log(xs[0], xs[1], xs.length);
const withNaN: number[] = [0 / 0, 0, 1.5];
console.log(withNaN[0], withNaN[1], withNaN[2]);
console.log(Number.isNaN(withNaN[0]), withNaN[1] === undefined);

// a value past f64's integer range reads the same as it does in bun —
// widening did not take precision away, because there was none to take
const big: number[] = [9007199254740993];
console.log(big[0]);
