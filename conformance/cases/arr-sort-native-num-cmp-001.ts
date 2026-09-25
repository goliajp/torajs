// `(a, b) => a - b` / `b - a` over number elements compares natively;
// the order must match the comparator's for every number, NaN and -0
// included, and every other comparator shape keeps the call.
const ints: number[] = [];
for (let i = 0; i < 50; i++) ints.push((i * 37) % 23 - 11);
ints.sort((a, b) => a - b);
console.log(ints.join(","));
ints.sort((a, b) => b - a);
console.log(ints.slice(0, 8).join(","));

const fs: number[] = [3.5, -0, 0, Infinity, -Infinity, 2.25, -1e300, 1e-300, 0.5, 0];
const asc = fs.slice();
asc.sort((x, y) => x - y);
console.log(asc.map((v) => (Object.is(v, -0) ? "-0" : String(v))).join(" "));
const desc = fs.toSorted((x, y) => y - x);
console.log(desc.map((v) => (Object.is(v, -0) ? "-0" : String(v))).join(" "));

const big: number[] = [];
for (let i = 0; i < 300; i++) big.push(((i * 7919) % 1000) / 8);
big.sort((a, b) => a - b);
let ok = true;
for (let i = 1; i < big.length; i++) if (big[i - 1] > big[i]) ok = false;
console.log(ok, big[0], big[299]);

let k = 1;
const scaled: number[] = [5, 1, 4, 2, 3];
scaled.sort((a, b) => (a - b) * k);
console.log(scaled.join(","));
const swapped: number[] = [5, 1, 4, 2, 3];
swapped.sort((a, b) => a - b + 0);
console.log(swapped.join(","));

const holes: number[] = [3, 1, 2];
holes[5] = 0;
holes.sort((a, b) => a - b);
console.log(holes.length, holes.join(","));
