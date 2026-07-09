// chunk 733 — owned-shape push args (closure literal / nested array
// literal / concat result) hand their +1 to the array; the balancing
// release closed a churn leak (probes P1/P6/P7). This fixture pins
// the VALUE semantics of each shape; the leak itself is pinned by
// the AOT churn probe protocol.
const nested: number[][] = [];
nested.push([1, 2]);
nested.push([3]);
console.log(nested.length, nested[0][1], nested[1][0]);
const strs: string[] = [];
strs.push("a" + "b".repeat(2));
console.log(strs[0]);
const fns: Array<() => number> = [];
fns.push(() => 7);
console.log(fns[0]());
