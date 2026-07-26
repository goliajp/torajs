// An out-of-range read answers `undefined`, and a binding that holds
// one is recorded where it is declared so the consumers around it know
// to check. A parameter has no such site — its value arrives from the
// caller's body, which is lowered separately — so nothing recorded it
// and the sentinel came back out as NaN inside the callee.

const q: number[] = [7];

// Both of these already answered `undefined`.
console.log(q[1]);
const x = q[1];
console.log(x);

// A named function's parameter.
function named(v: number): void {
  console.log(v);
}
named(q[1]);

// An arrow's, which reaches its lifted declaration under a synthetic
// name while the call site still spells the binding.
const arrow = (v: number) => {
  console.log(v);
};
arrow(q[1]);

// Through a re-binding of that arrow.
const alias = arrow;
alias(q[1]);

// Handing the parameter back out passes the sentinel along.
function passthrough(v: number): number {
  return v;
}
console.log(passthrough(q[1]));

// The other shapes that answer the same sentinel.
function taker(v: number): void {
  console.log(v);
}
taker(q.at(9));
taker([1, 2, 3].find((n) => n > 99));
const empty: number[] = [];
taker(empty.pop());
taker(empty.shift());

// An in-range read is untouched, and so is a second parameter nobody
// taints.
function pair(a: number, b: number): void {
  console.log(a, b);
}
pair(q[0], 2.5);
pair(q[5], 2.5);
