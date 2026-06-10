// W1 (ann-width RFC) — module-wide number-slot width inference.
// Each block locks one member of the repro family that the old
// per-site heuristics mishandled (silent truncation or compile abort).

// R1 — fract return through a `: number` ret slot (was: printed 0).
function fractRet(): number {
  return 0.5;
}
console.log(fractRet());

// R2 — int-literal init, later f64 assignment (was: rc=134 abort).
function accHalf(): number {
  let acc: number = 0;
  acc = acc + 0.5;
  return acc;
}
console.log(accHalf());

// S6 — same shape without the annotation (was: rc=134 abort).
function accHalfNoAnn(): number {
  let acc = 0;
  acc = acc + 0.5;
  return acc;
}
console.log(accHalfNoAnn());

// R3 — frem + fdiv in one function (was: rc=134 abort).
function oddCore(x: number): number {
  let n: number = x;
  while (n % 2 === 0) {
    n = n / 2;
  }
  return n;
}
console.log(oddCore(12));

// S1 — call-site fract arg into an int-shaped param (was: printed 1).
function addOne(x: number): number {
  return x + 1;
}
console.log(addOne(0.5));

// S2 — same through a slot-typed arg (was: printed 2).
function identNum(x: number): number {
  return x;
}
let vHalf: number = 2.5;
console.log(identNum(vHalf));

// S5 — int ret feeding a binding later divided (was: rc=134 abort).
function seven(): number {
  return 7;
}
let q: number = seven();
q = q / 4;
console.log(q);

// Narrow guard — pure-int domains must keep the i64 representation;
// these print identically either way but pin the no-poison paths.
function popcountish(x: number): number {
  let n: number = x;
  let count: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count = count + 1;
  }
  return count;
}
console.log(popcountish(255));

function gcdish(a: number, b: number): number {
  while (b !== 0) {
    let t: number = b;
    b = a % b;
    a = t;
  }
  return a;
}
console.log(gcdish(48, 18));
