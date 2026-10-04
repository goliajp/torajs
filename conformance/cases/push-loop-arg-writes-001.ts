// Fill loops whose push argument runs code: a call that pushes to the
// same array, one that moves the loop bound, an assignment that does.
// Each append has to land where the runtime push would put it, and the
// array has to hold every one of them.
function g(a: number[]): number {
  a.push(99);
  return 1;
}
function h(): number[] {
  const xs: number[] = [];
  for (let i = 0; i < 3; i++) xs.push(g(xs));
  return xs;
}
console.log(h().join(","));

// pushes twice per call, past whatever the loop reserved
function g3(a: number[]): number {
  a.push(7);
  a.push(8);
  return 1;
}
function h3(): number[] {
  const xs: number[] = [];
  for (let i = 0; i < 1000; i++) xs.push(g3(xs));
  return xs;
}
const r = h3();
let s = 0;
for (let j = 0; j < r.length; j++) s = s + r[j];
console.log(r.length, s, r[0], r[1], r[2], r[2999]);

// the bound grows from 4 to 200000 inside the first argument
let n = 4;
function bump(): number {
  n = 200000;
  return 1;
}
const ys: number[] = [];
for (let i = 0; i < n; i++) ys.push(bump());
let t = 0;
for (let j = 0; j < ys.length; j++) t = t + ys[j];
console.log(ys.length, t);

// the same, written as an assignment in the argument
let m = 100;
const zs: number[] = [];
for (let i = 0; i < m; i++) zs.push((m = 300));
console.log(zs.length, zs[299]);

// and in the while shape
let k = 0;
let lim = 2;
const ws: number[] = [];
while (k < lim) {
  ws.push((lim = 5));
  k = k + 1;
}
console.log(ws.length, ws.join(","));
