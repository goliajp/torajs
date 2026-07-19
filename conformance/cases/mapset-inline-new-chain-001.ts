// Method calls chained directly on a `new Map(...)` / `new Set(...)`
// expression: the dispatch receiver-type hint now recognizes a
// New-shaped receiver (it previously fell through to the array-HO
// lane and died with "forEach on non-array receiver type Set").
new Set([4, 5]).forEach(function (v: number) {
  console.log(v);
});
new Map([["a", 1]]).forEach(function (v: number, k: string) {
  console.log(k, v);
});
console.log(new Set([1, 2, 3]).has(2));
console.log(new Map([["k", 7]]).get("k"));
const u = new Set([1, 2]).union(new Set([3]));
console.log(u.size);
new Map<string, number>([["x", 5]]).forEach(function (v) {
  console.log(v * 2);
});
