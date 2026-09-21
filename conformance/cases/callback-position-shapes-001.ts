// The last callback positions, closing the family: Array.from /
// %TypedArray%.from's mapfn (§23.1.2.1 step 4.a), the
// getOrInsertComputed callbacks on Map and WeakMap, and the
// %Iterator.prototype% helpers (§27.1.4.x step 2).
//
// Same defect as the rest — a closure-cell probe standing in for
// IsCallable — but three different repairs, because what each site
// can carry differs:
//
//   * the helpers already store the callable in their cell and call
//     it once per step, so they ask the shared predicate and call
//     the shared kernel; no pair is carried at all;
//   * Array.from's mapfn now travels as the VALUE, since its thisArg
//     is per-call and a pair could never have carried that either;
//   * the upsert callbacks and the eager helpers resolve once and
//     hold the result for the walk, thunk included.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const dbl: any = new Proxy(((x: number) => x * 2) as any, {});
const big: any = new Proxy(((x: number) => x > 1) as any, {});
const sum: any = new Proxy(((a: number, b: number) => a + b) as any, {});
const src: any = [1, 2, 3];

console.log("from array   ", attempt(() => Array.from(src, dbl).join(",")));
console.log("from set     ", attempt(() => Array.from(new Set([1, 2]) as any, dbl).join(",")));
console.log("from string  ", attempt(() =>
  Array.from("ab" as any, new Proxy(((c: string) => c + "!") as any, {})).join(",")));
console.log("from thisArg ", attempt(() => {
  const t: any = { k: 10, go(this: any, x: number) { return this.k + x; } };
  return Array.from([1, 2] as any, new Proxy(t.go as any, {}), t).join(",");
}));
console.log("TA from      ", attempt(() =>
  Array.from(Uint8Array.from([1, 2] as any, dbl) as any).join(",")));
console.log("from plain   ", attempt(() => Array.from(src, (x: number) => x * 2).join(",")));
console.log("from refuses ", attempt(() => Array.from(src, 5 as any)));

console.log("Map upsert   ", attempt(() =>
  (new Map() as any).getOrInsertComputed("a", new Proxy(((k: string) => k + "!") as any, {}))));
console.log("Weak upsert  ", attempt(() =>
  (new WeakMap() as any).getOrInsertComputed({}, new Proxy((() => 7) as any, {}))));
console.log("upsert plain ", attempt(() =>
  (new Map() as any).getOrInsertComputed("a", (k: string) => k + "!")));
console.log("upsert refuse", attempt(() => (new Map() as any).getOrInsertComputed("a", 5)));

console.log("iter map     ", attempt(() =>
  Array.from(([1, 2] as any).values().map(dbl)).join(",")));
console.log("iter filter  ", attempt(() =>
  Array.from(([1, 2, 3] as any).values().filter(big)).join(",")));
console.log("iter flatMap ", attempt(() =>
  Array.from(([1, 2] as any).values().flatMap(new Proxy(((x: number) => [x, x]) as any, {})))
    .join(",")));
console.log("iter reduce  ", attempt(() => ([1, 2, 3] as any).values().reduce(sum, 0)));
console.log("iter some    ", attempt(() => ([1, 2] as any).values().some(big)));
console.log("iter every   ", attempt(() => ([1, 2] as any).values().every(big)));
console.log("iter find    ", attempt(() => ([1, 2] as any).values().find(big)));
console.log("iter forEach ", attempt(() => {
  const o: number[] = [];
  ([1, 2] as any).values().forEach(new Proxy(((x: number) => { o.push(x); }) as any, {}));
  return o.join(",");
}));
console.log("iter plain   ", attempt(() =>
  Array.from(([1, 2] as any).values().map((x: number) => x * 2)).join(",")));
console.log("iter refuses ", attempt(() => ([1] as any).values().map(5)));
// A throwing proxy callback propagates and closes the underlying.
console.log("iter throws  ", attempt(() =>
  Array.from(([1, 2] as any).values().map(new Proxy((() => {
    throw new Error("b");
  }) as any, {})))));
// A class is callable everywhere here too, and refused when called.
class K {}
console.log("from class   ", attempt(() => Array.from(src, K as any)));
console.log("iter class   ", attempt(() => Array.from(([1] as any).values().map(K))));
console.log("upsert class ", attempt(() => (new Map() as any).getOrInsertComputed("a", K)));
