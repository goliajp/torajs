// Same bound-read-once defect on the `while` lane.
let n: number = 0;
function bnd(): number { n = n + 1; return 3; }
function f(): number {
  let xs: number[] = [];
  let i: number = 0;
  while (i < bnd()) { xs.push(i); i++; }
  return xs.length;
}
let r: number = f();
console.log(r, n);
