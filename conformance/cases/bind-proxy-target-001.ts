// §20.2.3.2 step 4 — a bound function's [[BoundTargetFunction]] is
// the value `bind` was called ON, and §10.4.1.1 step 5 then calls
// THAT. When the value is a proxy, both halves were wrong:
//
//   * the dispatch forwarded `bind` to the proxy's target, so the
//     bound function held the target function and every trap was
//     gone — `p.bind(null, 5)()` on a proxy with an `apply` trap
//     answered undefined instead of running it;
//   * the bound cell's own call path spelled §7.2.3 out again and
//     named two of the three shapes (closure, class), so even a
//     proxy placed there directly would have fallen to
//     "not a function".
//
// Both now read the shared askers: §20.2.3.2 step 2's gate is
// `construct::is_callable`, and §10.4.1.1 step 5 is the ordinary
// explicit-`this` call kernel, which holds all three shapes and runs
// the `apply` trap.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
function add(a: number, b: number): number {
  return a + b;
}
const plain: any = new Proxy(add as any, {});
const trapped: any = new Proxy(add as any, {
  apply: (_t: any, _th: any, a: any) => a[0] * 100 + a[1],
});

console.log("apply trap runs  ", attempt(() => trapped.bind(null, 5)(2)));
console.log("trap, all bound  ", attempt(() => trapped.bind(null, 5, 2)()));
console.log("plain proxy bind ", attempt(() => plain.bind(null, 1)(2)));
console.log("no partial args  ", attempt(() => plain.bind(null)(3, 4)));
console.log("rebind a bound   ", attempt(() => plain.bind(null, 1).bind(null, 2)()));
// §20.2.3.2 steps 5-14 read the target's length and name — through
// the proxy, which is what makes them observable at all.
console.log("bound name       ", attempt(() => plain.bind(null, 1).name));
console.log("bound length     ", attempt(() => plain.bind(null, 1).length));
console.log("typeof bound     ", attempt(() => typeof plain.bind(null)));
// A class object is callable, so it binds — and the bound call still
// raises §10.2.1 step 2, from the one place that rule lives.
class K {}
console.log("class binds      ", attempt(() => typeof (K as any).bind(null)));
console.log("bound class call ", attempt(() => (K as any).bind(null)()));
console.log("bound proxy class", attempt(() => (new Proxy(K as any, {}) as any).bind(null)()));
// An intrinsic constructs instead (§20.5.1.1).
console.log("bound Error      ", attempt(() => (Error as any).bind(null)("m").message));
// Not callable, so step 2 refuses it.
console.log("bound non-callable", attempt(() => (new Proxy({} as any, {}) as any).bind(null)));
// Plain and builtin targets are unmoved.
console.log("plain fn bind    ", attempt(() => {
  const b: any = add.bind(null, 1);
  return b(2) + "/" + b.name + "/" + b.length;
}));
console.log("builtin bind     ", attempt(() => (Array.prototype.join as any).bind([1, 2, 3])("-")));
