// The element of a `number[]` has a range — every value written into
// it has one — and the load that reads it back is the one place that
// range could not be seen. Two halves make it visible: the element
// stops being widened to F64 when every index read on it is proven
// in bounds at BOTH ends, and the interval lattice gains a point per
// allocation so the load answers with the join of the writes.
//
// Neither half is observable on its own. What is observable is that
// none of it may change an answer: an out-of-range read still owes
// `undefined`, a sum past 2^53 still rounds the way an f64 does, and
// an array the analysis cannot follow still adds up.

// the shape the whole thing is for: grow, then walk forward
const xs: number[] = [];
let i: number = 0;
while (i < 200) {
  xs.push(i);
  i = i + 1;
}
let sum: number = 0;
let j: number = 0;
while (j < xs.length) {
  sum = sum + xs[j];
  j = j + 1;
}
console.log(sum, xs.length);

// one unproven read anywhere in the class widens the whole class back
const ys: number[] = [];
let a: number = 0;
while (a < 4) {
  ys.push(a * 10);
  a = a + 1;
}
let t: number = 0;
let b: number = 0;
while (b < ys.length) {
  t = t + ys[b];
  b = b + 1;
}
console.log(t, ys[9], ys[-1]);

// negative elements put the point below zero
const ns: number[] = [-5, -1, 0, 3];
let nt: number = 0;
let k: number = 0;
while (k < ns.length) {
  nt = nt + ns[k];
  k = k + 1;
}
console.log(nt);

// a sum that leaves the safe-integer range must round like an f64
const big: number[] = [];
let g: number = 0;
while (g < 6) {
  big.push(4503599627370496);
  g = g + 1;
}
let bt: number = 0;
let h: number = 0;
while (h < big.length) {
  bt = bt + big[h];
  h = h + 1;
}
console.log(bt, bt + 1 === bt);

// fractional writes keep the element f64 whatever the reads prove
const fs: number[] = [0.5, 1.25, 2.25];
let ft: number = 0;
let p: number = 0;
while (p < fs.length) {
  ft = ft + fs[p];
  p = p + 1;
}
console.log(ft);

// handing the array to a function puts it out of reach of the
// element point; the answer is the same either way
function total(src: number[]): number {
  let s: number = 0;
  let q: number = 0;
  while (q < src.length) {
    s = s + src[q];
    q = q + 1;
  }
  return s;
}
const es: number[] = [7, 8, 9];
console.log(total(es), es[3]);

// a step larger than one is still an induction: every value the
// counter takes is non-negative, so these reads are proven too
const zs: number[] = [1, 2, 3, 4, 5, 6];
let out: string = "";
let z: number = 0;
while (z < zs.length) {
  out = out + String(zs[z]) + " ";
  z = z + 2;
}
console.log(out);

// a body that can take the counter backwards is not an induction the
// guard can ride: the guard settles `< length` on every iteration and
// nothing settles `>= 0`, so the reads keep their checked form and the
// negative ones answer `undefined`
const vs: number[] = [10, 20, 30];
let e: number = 0;
let once: boolean = true;
let vout: string = "";
while (e < vs.length) {
  vout = vout + String(vs[e]) + " ";
  if (e === 2 && once) {
    once = false;
    e = e - 5;
  }
  e = e + 1;
}
console.log(vout);

// a counter the loop's own `let` binds is out of everyone else's
// reach, so a sibling loop reusing the name does not block it
function twice(n: number): number {
  const src: number[] = [];
  for (let i: number = 0; i < n; i = i + 1) {
    src.push(i);
  }
  let acc: number = 0;
  for (let i: number = 0; i < src.length; i = i + 1) {
    acc = acc + src[i];
  }
  return acc;
}
console.log(twice(50));

// and one unguarded read on the same array still widens its class
function twiceOob(n: number): string {
  const src2: number[] = [];
  for (let i: number = 0; i < n; i = i + 1) {
    src2.push(i * 2);
  }
  let acc: number = 0;
  for (let i: number = 0; i < src2.length; i = i + 1) {
    acc = acc + src2[i];
  }
  return String(acc) + " " + String(src2[999]);
}
console.log(twiceOob(4));
