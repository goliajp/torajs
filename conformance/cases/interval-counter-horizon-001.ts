// A counter compared against a number that arrives at run time gets
// no bound from the compare. It is still bounded: it starts from its
// initial value and moves by a small step at a time. The elements an
// array is filled with carry that bound, and the sum over them drops
// its per-element range check only where the bound says the elements
// stay exact. Where they do not, the sum must round as an f64 does.

const n = parseInt(process.argv[2] ?? "12", 10) || 12;

function sumOf(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) {
    s = s + xs[i];
  }
  return s;
}

// the counter itself: 0 .. n-1
const a: number[] = [];
for (let i = 0; i < n; i++) {
  a.push(i);
}
let sa = 0;
for (let j = 0; j < a.length; j++) {
  sa = sa + a[j];
}
console.log(a.length, sa, sumOf(a));

// a negative offset puts elements below zero
const b: number[] = [];
for (let i = 0; i < n; i++) {
  b.push(i - 10);
}
let sb = 0;
for (let j = 0; j < b.length; j++) {
  sb = sb + b[j];
}
console.log(b.join(","), sb);

// a counter stepping down from zero
const c: number[] = [];
let k = 0;
while (k > -n) {
  c.push(k);
  k = k - 1;
}
let sc = 0;
for (let j = 0; j < c.length; j++) {
  sc = sc + c[j];
}
console.log(c.length, sc, k);

// elements just under 2^53 whose sum leaves the exact range
const top = parseInt("9007199254740000", 10);
const d: number[] = [];
for (let i = 0; i < n; i++) {
  d.push(top + i);
}
let sd = 0;
for (let j = 0; j < d.length; j++) {
  sd = sd + d[j];
}
console.log(sd, sd === sumOf(d));

// elements past 2^53, where adding the counter rounds
const big = parseInt("18014398509481984", 10);
const e: number[] = [];
for (let i = 0; i < n; i++) {
  e.push(big + i);
}
let se = 0;
for (let j = 0; j < e.length; j++) {
  se = se + e[j];
}
console.log(e[1], e[2], e[3], e[5], se);

// big elements written by a counter's step, summed past 2^53
const f: number[] = [];
let p = 4503599627370000;
for (let i = 0; i < n; i++) {
  f.push(p);
  p = p + 32;
}
let sf = 0;
for (let j = 0; j < f.length; j++) {
  sf = sf + f[j];
}
console.log(sf, f[f.length - 1]);
