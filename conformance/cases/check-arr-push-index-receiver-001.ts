// Chunk 697 — `xs[i].push(v)` index-read receiver (pre-existing
// loud gap isolated by the chunk-695 probe: the push lane only knew
// Ident / global / obj.field receivers). B1 fixed the arr cell
// across grow, so the borrowed elem read IS the receiver — no
// outer-slot write-back exists to miss; the 1000-push lane crosses
// many realloc boundaries and the outer read-back stays live.
const z: number[][] = [[1], [2]];
z[0].push(9);
console.log(z);
console.log(z[0].length);
const g: number[][] = [[]];
for (let i = 0; i < 1000; i++) {
  g[0].push(i);
}
console.log(g[0].length, g[0][999]);
// refcounted elems
const s: string[][] = [["a"]];
s[0].push("bb");
console.log(s);
// Any inner arrays route the tagged-slot helper
const anyz: any[][] = [[1, "x"]];
anyz[0].push(true);
console.log(anyz[0]);
// spec §22.1.3.20 — push answers the new length
const r: number[][] = [[5]];
const n = r[0].push(6);
console.log(n);
// non-literal index
const k = 1;
const w: number[][] = [[1], [2]];
w[k].push(7);
console.log(w);
