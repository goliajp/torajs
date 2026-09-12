// rotation 638 (plan 636-15 ①) — §14.7.5.7: a `let` head's per-iteration
// binding is mutable. The body may assign the loop variable in every
// source lane; a written variable is bound OWNED (retained at init,
// dropped each iteration), so a write in an untaken branch leaves a
// binding the frame close may still release. oracle = bun.
const nums = [1, 2, 3];
const out: number[] = [];
for (let z of nums) { z = z * 10; out.push(z); }
console.log("numbers", out.join(","));

// heap elements, conditional write, allocation churn between iterations
const words = ["alpha", "beta", "gamma", "delta"];
const seen: string[] = [];
for (let w of words) {
  if (w.length === 5) { w = w.toUpperCase(); }
  const pad = "-".repeat(3) + w;
  seen.push(pad.slice(3));
}
console.log("strings", seen.join(","), words.join(","));

const objs = [{ n: 1 }, { n: 2 }];
let total = 0;
for (let o of objs) {
  if (o.n === 2) { o = { n: 20 }; }
  total += o.n;
}
console.log("objects", total, objs[1].n);

const rec: any = { a: 1, b: 2 };
const keys: string[] = [];
for (let k in rec) { k = k + "!"; keys.push(k); }
console.log("for-in", keys.join(","));

// captured by a closure: the written per-iteration binding is a capture
// box, one per iteration. (`const fs: (() => number)[]` — a typed closure
// array — reads a pointer for ANY for-of loop variable, written or not,
// on the baseline too; registered separately, so `any[]` here.)
const fs: any[] = [];
for (let z of nums) { z = z + 1; fs.push(() => z); }
console.log("captured", fs[0](), fs[1](), fs[2]());
const gs: any[] = [];
for (let z of nums) { gs.push(() => z); z = z * 2; }
console.log("captured-after", gs[0](), gs[1](), gs[2]());

const chars: string[] = [];
for (let c of "ab") { c = c + c; chars.push(c); }
console.log("string source", chars.join(","));

const m = new Map<number, number>([[1, 2], [3, 4]]);
const ms: number[] = [];
for (let e of m) { e = [e[0] * 10, e[1]]; ms.push(e[0] + e[1]); }
console.log("map", ms.join(","));

class CounterIter {
  i: number;
  n: number;
  constructor(n: number) { this.i = 0; this.n = n; }
  next(): { value: number; done: boolean } { this.i++; return { value: this.i, done: this.i > this.n }; }
}
class Counter {
  n: number;
  constructor(n: number) { this.n = n; }
  [Symbol.iterator](): CounterIter { return new CounterIter(this.n); }
}
const cs: number[] = [];
for (let v of new Counter(3)) { if (v > 1) { v = v * 100; } cs.push(v); }
console.log("iterator", cs.join(","));

const anySrc: any = ["x", "y"];
const as: string[] = [];
for (let a of anySrc) { a = a + "?"; as.push(a); }
console.log("any", as.join(","));
