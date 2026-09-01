// Owned temps alive across a throwing sibling in a direct call's
// argument list and in array literals (rotation 549 — 549-01 batch 1):
// the values must still be built / thrown exactly as bun does, and the
// churn loops keep answering the same (the lowering parks the temps
// for the throw path; pre-549 they were stranded, 175-252MB / 600k).
const boom = (): any => { throw new Error("x"); };
const f2 = (a: any, b: any) => a;
const f3 = (a: any, b: any, c: any) => c;
const mk = (n: number): any => ({ n });

let counts = [0, 0, 0, 0, 0, 0];
for (let i = 0; i < 200; i++) {
  try { f2({} as any, boom()); } catch { counts[0]++; }
  try { f3(mk(i), [i] as any, boom()); } catch { counts[1]++; }
  try { const a = [{} as any, boom()]; } catch { counts[2]++; }
  try { const a = [mk(i), mk(i + 1), boom()]; } catch { counts[3]++; }
  try { const a: any[] = [i, "s" + i, boom()]; } catch { counts[4]++; }
  try { const a = [[1, 2], [], boom()]; } catch { counts[5]++; }
}
console.log(counts.join(","));

// the normal paths still answer the values
console.log(JSON.stringify(f2({ a: 1 } as any, 2)), f3(1, 2, mk(3)).n);
console.log(JSON.stringify([{ a: 1 } as any, 2]), JSON.stringify([mk(1), mk(2)]));
const het: any[] = [1, "s", mk(4)];
console.log(JSON.stringify(het), JSON.stringify([[1, 2], [], [3]]));
