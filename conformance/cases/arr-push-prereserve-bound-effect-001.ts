// The pre-reserve lanes read the loop bound once, above the loop, and
// then trust that value. They justified it with "the cond reads it on
// every iteration unchanged" — true of the expression, and silent
// about its value. A bound that calls is read one extra time the
// program never wrote, so a counter the call bumps answers one too
// high. It fired even with no array eligible for a reserve, because
// the bound was lowered ahead of the filter that rejects them.
let n: number = 0;
function bnd(): number { n = n + 1; return 3; }
function f(): number {
  let xs: number[] = [];
  for (let i: number = 0; i < bnd(); i++) { xs.push(i); }
  return xs.length;
}
let r: number = f();
console.log(r, n);
let m: number = 0;
function bnd2(): number { m = m + 1; return 3; }
let top: number[] = [];
for (let i: number = 0; i < bnd2(); i++) { top.push(i); }
console.log(top.length, m);
