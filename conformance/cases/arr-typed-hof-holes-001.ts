// The §23.1.3 callback methods skip the holes of a TYPED source — a write
// past the end or a longer length leaves them — and reduce without an
// initial value seeds from the first present element.
let y = [1.5, 2.5];
y[4] = 3.5;
let c = 0;
y.forEach(() => { c++; });
console.log(c, y.reduce((a, v) => a + v, 0), y.reduce((a, v) => a + v));
console.log(y.filter(() => true), y.reduceRight((a, v) => a + v, 0));
let g = [1, 2];
g.length = 5;
let d = 0;
g.forEach(() => { d++; });
console.log(d, g.filter(() => true), g.reduceRight((a, v) => a + v));
let h: number[] = [];
h[3] = 7;
console.log(h.reduce((a, v) => a * 10 + v), h.reduce((a, v, i) => a + i, 100));
let e: number[] = [];
e.length = 3;
try { e.reduce((a, v) => a + v); } catch (x) { console.log((x as Error).constructor.name); }
let s = ["a", "b"];
s[4] = "e";
const idx: number[] = [];
s.forEach((v, i) => { idx.push(i); });
console.log(s.filter(() => true), s.reduce((a, v) => a + v), s.reduceRight((a, v) => a + v, "!"), idx);
const o = { k: 10 };
let t = 0;
g.forEach(function (this: { k: number }, v) { t += v * this.k; }, o);
console.log(t);
try {
  y.filter((v) => { if (v > 3) throw new Error("boom"); return true; });
} catch (x) { console.log((x as Error).message); }
const z = [1.5, 2.5, 3.5];
console.log(z.reduce((a, v) => a + v, 0), z.map((v) => v + 1), z.filter((v) => v > 2));
