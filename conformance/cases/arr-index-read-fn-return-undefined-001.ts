// Reading past the end answers `undefined` (ES §10.4.2.1), and handing
// that read straight back out of a function has to answer the same
// thing. The read written at the call site always did; the one written
// as a function's return didn't — the caller read the sentinel as a
// plain value and printed NaN.
//
// It takes an element class wide enough to carry that answer. An
// all-integral `number[]` has no bit pattern to spare and answers the
// slot's zero there instead, which is a separate gap; a `find` seed or
// a fractional value is enough to widen it.

const xs: number[] = [1, 2, 3];
xs.find((x: number): boolean => x > 2);

function pick(src: number[], i: number): number {
  return src[i];
}
console.log(pick(xs, 1), pick(xs, 7));

// the same read written at the call site — right all along
console.log(xs[1], xs[7]);

// parked in a local on the way out
function pick_via_local(src: number[], i: number): number {
  const v = src[i];
  return v;
}
console.log(pick_via_local(xs, 0), pick_via_local(xs, 9));

// from inside a branch
function first_or(src: number[], i: number): number {
  if (i < 0) {
    return src[0];
  }
  return src[i];
}
console.log(first_or(xs, 2), first_or(xs, 5));

// a fractional element widens the class without a method seed
const fs: number[] = [0.5, 1.5];
function pickf(src: number[], i: number): number {
  return src[i];
}
console.log(pickf(fs, 1), pickf(fs, 4));

// arithmetic on the answer is a plain NaN, not the sentinel
console.log(pick(xs, 7) + 1);

// and it still compares as undefined does
console.log(pick(xs, 7) === undefined);
console.log(pick(xs, 0) === 1);
