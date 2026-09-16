// Every source lane the per-element step has to resume across calls:
// an array and a string ride the resume index, a Map / Set derives a
// cursor on its first step, a generator is a user iterator.
const a: any = {};
[a["x"], a["y"]] = [10, 20];
console.log(a.x, a.y);
const s: any = {};
[s["p"], s["q"]] = "hi";
console.log(s.p, s.q);
const m: any = {};
[m["k"], m["l"]] = new Map([
  [1, 2],
  [3, 4],
]);
console.log(JSON.stringify(m.k), JSON.stringify(m.l));
const t: any = {};
[t["u"], t["v"]] = new Set([3, 4]);
console.log(t.u, t.v);
function* g(): any {
  yield 1;
  yield 2;
}
const o: any = {};
[o["a"], o["b"]] = g();
console.log(o.a, o.b);
