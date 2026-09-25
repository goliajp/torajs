// a fill loop that pushes the same array more than once a trip needs
// that many slots a trip; the fast path reserved one and wrote past
// the buffer.
const xs: string[] = [];
for (let i = 0; i < 4; i++) {
  xs.push("a" + i);
  xs.push("b" + i);
  xs.push("c" + i);
}
const ys: string[] = [];
for (let i = 0; i < 64; i++) ys.push("y" + i);
console.log(xs.length, xs.join(","));
console.log(ys.length, ys[0], ys[63]);

const ns: number[] = [];
for (let i = 0; i < 1000; i = i + 1) {
  ns.push(i);
  ns.push(-i);
}
let s = 0;
for (let i = 0; i < ns.length; i = i + 1) s = s + ns[i] * (i % 3);
console.log(ns.length, s, ns[1998], ns[1999]);

const p: number[] = [];
const q: number[] = [];
let j = 0;
while (j < 300) {
  p.push(j);
  q.push(j * 2);
  p.push(j + 0.5);
  j = j + 1;
}
console.log(p.length, q.length, p[599], q[299]);

let big: number[] = [7];
for (let i = 0; i < 5000; i++) {
  big.push(i);
  big.push(i);
  big.push(i);
  big.push(i);
}
console.log(big.length, big[0], big[20000]);
