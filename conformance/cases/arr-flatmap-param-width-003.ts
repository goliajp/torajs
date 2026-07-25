// The element flatMap hands a callback must arrive at the width that
// callback's parameter was compiled for.
//
// An element's width and a parameter's width answer to two different
// classes, so they can legitimately disagree: a callback shared with a
// fractional array is compiled to take an f64, while this receiver's
// elements are still integers. The shared higher-order loop converts on
// that mismatch; flatMap built its own call and skipped it, and the
// integer element reaching an f64 parameter aborted register
// allocation:
//
//   not yet supported: materialize_operand_fpr called on ValueId
//   holding Gpr
//
// Loud rather than silent, but the same disagreement the neighbouring
// cases in this family are about.

function pair(x: number): number[] {
  return [x + 0.5];
}

// the integer receiver is what widens nothing, and used to abort
const xs: number[] = [1, 2];
console.log(xs.flatMap(pair)[0], xs.flatMap(pair)[1]);

// the same callback against a fractional receiver, which is what
// widened its parameter in the first place
const ys: number[] = [1.5, 2.5];
console.log(ys.flatMap(pair)[0], ys.flatMap(pair)[1]);

// order reversed — the fractional receiver seen first
function twice(x: number): number[] {
  return [x, x];
}
const fs: number[] = [0.5];
const is: number[] = [3];
console.log(fs.flatMap(twice)[0], is.flatMap(twice)[0]);

// a scalar-returning callback shared the same way
function inc(x: number): number {
  return x + 1;
}
console.log(is.flatMap(inc)[0], fs.flatMap(inc)[0]);
