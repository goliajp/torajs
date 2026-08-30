// The reserved capacity is `len + bound` in i64. A bound that is inert
// and loop-invariant can still be an f64 — `number` says nothing about
// which width it landed in — and the backend refused to materialise it
// into the add. The width is checked on the lowered operand now.
function f(): number {
  let n: number = 10;
  let xs: number[] = [];
  for (let i: number = 0; i < n / 2; i++) { xs.push(i); }
  return xs.length;
}
function g(): number {
  let n: number = 10;
  let xs: number[] = [];
  let i: number = 0;
  while (i < n / 2) { xs.push(i); i++; }
  return xs.length;
}
console.log(f(), g());
