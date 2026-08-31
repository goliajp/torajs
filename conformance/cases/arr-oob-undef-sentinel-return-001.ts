// The fall-through table answers which returns can hand back the
// `undefined` sentinel rather than an ordinary value, and it is a
// second predicate over the same shapes as the in-body one. It read
// the bare shapes only, so `return true ? xs[9] : 0` handed the
// sentinel on as a plain value and the caller printed NaN — while
// `return xs[9]` one line above answered `undefined`. Not even the
// cast was on this one's list.
let xs: number[] = [1, 2, 3];

function ret_plain(): number {
  return xs[9];
}
function ret_ternary(): number {
  return true ? xs[9] : 0;
}
function ret_seq(): number {
  return (0, xs[9]);
}
function ret_cast(): number {
  return xs[9] as number;
}
function ret_nullish(): number {
  return xs[0] ?? xs[9];
}
console.log(ret_plain());
console.log(ret_ternary());
console.log(ret_seq());
console.log(ret_cast());
console.log(ret_nullish());
console.log(typeof ret_ternary());
console.log(ret_seq() === undefined);

// In-range returns through the same wrappers stay numbers.
function ret_ok(): number {
  return true ? xs[1] : 0;
}
console.log(ret_ok());
console.log(typeof ret_ok());
console.log(ret_ok() + 1);
