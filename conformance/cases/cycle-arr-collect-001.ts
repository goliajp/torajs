// chunk 614 — cycle collector reclaims arr-participating cycles:
// pure Array<Any> self-cycle (arr drop now registers cycle roots),
// obj <-> any[] field cycle (non-empty literal into an Any-elem
// field allocates through arr_alloc_any so the walk can cross it),
// and the collect_white re-drop guard keeps the pass crash-free.
class Node {
  arr: any[] = [1];
}
let acc = 0;
for (let i = 0; i < 30000; i++) {
  const a: any[] = [i];
  a.push(a);
  const n = new Node();
  n.arr.push(n);
  acc += a.length + n.arr.length;
}
console.log(acc);
