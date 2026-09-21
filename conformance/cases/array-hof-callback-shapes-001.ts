// §23.1.3.x step 2 is IsCallable(callbackfn) — §7.2.3, the same
// question `typeof` answers. Every higher-order kernel takes its
// callback as a raw `(env, entry)` pair, because that is the one
// calling convention a per-element loop can use without asking
// questions, and the pair came from a CLOSURE-CELL probe. So the
// gate in front of each loop was asking "is this a closure?", and
// the other two shapes §7.2.3 names — a callable Proxy, a class
// object — were turned away: `[1, 2].map(new Proxy(fn, {}))` said
// "value is not a function" about a value `typeof` calls a function.
//
// Swapping the gate's predicate would not have helped: the loop
// would then get a null entry and jump to 0. What the loop needs is
// a pair, so a shape without one now gets a thunk — a bound cell
// with no partial arguments, whose dual entry performs §10.4.1.1
// step 5's ordinary call on the target. The loop cannot tell the
// difference. A real closure still answers its own pair, so the
// common path is unchanged and nothing is allocated for it.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const a: any = [1, 2, 3];
const dbl: any = new Proxy(((x: number) => x * 2) as any, {});
const big: any = new Proxy(((x: number) => x > 1) as any, {});
const desc: any = new Proxy(((x: number, y: number) => y - x) as any, {});
const sum: any = new Proxy(((x: number, y: number) => x + y) as any, {});
// An `apply` trap must run for each element, not be bypassed.
const trapped: any = new Proxy((() => 0) as any, {
  apply: (_t: any, _th: any, args: any) => args[0] * 100,
});

console.log("map       ", attempt(() => a.map(dbl).join(",")));
console.log("map trap  ", attempt(() => a.map(trapped).join(",")));
console.log("filter    ", attempt(() => a.filter(big).join(",")));
console.log("forEach   ", attempt(() => {
  const o: number[] = [];
  a.forEach((x: any) => o.push(x));
  const o2: number[] = [];
  a.forEach(new Proxy(((x: number) => { o2.push(x); }) as any, {}));
  return o.join(",") + "|" + o2.join(",");
}));
console.log("every     ", attempt(() => a.every(big)));
console.log("some      ", attempt(() => a.some(big)));
console.log("find      ", attempt(() => a.find(big)));
console.log("findIndex ", attempt(() => a.findIndex(big)));
console.log("findLast  ", attempt(() => a.findLast(big)));
console.log("findLastIx", attempt(() => a.findLastIndex(big)));
console.log("reduce    ", attempt(() => a.reduce(sum, 0)));
console.log("reduceRight", attempt(() => a.reduceRight(sum, 0)));
console.log("flatMap   ", attempt(() => a.flatMap(new Proxy(((x: number) => [x, x]) as any, {})).join(",")));
const unsorted: any = [1, 3, 2];
console.log("sort      ", attempt(() => unsorted.sort(desc).join(",")));
console.log("toSorted  ", attempt(() => a.toSorted(desc).join(",")));
// thisArg reaches the callback either way — a closure through the
// kernel's receiver channel, a thunk because it carries it.
const self: any = { k: 10, go(this: any, x: number) { return this.k + x; } };
console.log("thisArg   ", attempt(() => a.map(new Proxy(self.go as any, {}), self).join(",")));
// A class object is callable, and §10.2.1 step 2 refuses the call —
// a TypeError, which is not the same as "not a function".
class K {}
console.log("class     ", attempt(() => a.map(K as any)));
// Genuinely not callable, still refused.
console.log("number    ", attempt(() => a.map(5 as any)));
console.log("object    ", attempt(() => a.map({} as any)));
console.log("proxy obj ", attempt(() => a.map(new Proxy({} as any, {}) as any)));
// sort's undefined comparator is the ToString default, untouched.
const unsorted2: any = [3, 1, 2];
console.log("sort dflt ", attempt(() => unsorted2.sort().join(",")));
// Recorded gap, not this change's: the TYPED lane's checker still
// refuses an `any` comparator on a typed array literal
// (`[1,3,2].sort(p)` — "expected Function([Number, Number], Number),
// got Any"), so these go through the any-typed receiver. The runtime
// under it now takes every callable shape.

console.log("toSorted d", attempt(() => a.toSorted().join(",")));
// Ordinary closures, unmoved.
console.log("plain     ", attempt(() =>
  a.map((x: number) => x * 2).join(",") + "|" + a.reduce((p: number, q: number) => p + q, 0)));
