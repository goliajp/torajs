// Owned temps alive across a throwing sibling field in object literals
// (rotation 549 — 549-01 batch 2): the fresh dynobj / the already
// lowered struct fields must still build exactly as bun does, and the
// churn loops keep answering the same (the lowering parks them for the
// throw path; pre-549 the any-lane literal stranded 214MB / 600k).
const boom = (): any => { throw new Error("x"); };
const mk = (n: number): any => ({ n });
const s = (n: number): string => "k" + n;

let counts = [0, 0, 0, 0, 0];
for (let i = 0; i < 200; i++) {
  try { const o: any = { a: mk(i), b: boom() }; } catch { counts[0]++; }
  try { const o = { a: mk(i), b: boom() }; } catch { counts[1]++; }
  try { const o: any = { a: { x: i }, b: [i, i + 1], c: s(i), d: boom() }; } catch { counts[2]++; }
  try { const o = { a: s(i), b: [mk(i)], c: boom() }; } catch { counts[3]++; }
  try { const o: any = { ...mk(i), b: boom() }; } catch { counts[4]++; }
}
console.log(counts.join(","));

// the normal paths still build
const a: any = { p: mk(1), q: { r: [2] }, s: s(3) };
console.log(JSON.stringify(a));
const b = { p: s(4), q: [mk(5)], r: { t: 6 } };
console.log(JSON.stringify(b));
const c: any = { ...mk(7), u: s(8) };
console.log(JSON.stringify(c));
