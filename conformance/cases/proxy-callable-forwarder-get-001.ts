// §7.3.2 — `p.m(…)` is `Call(GetV(p, "m"), p, args)`, so the NAME
// goes through [[Get]] before anything shape-keyed gets a say. The
// proxy dispatch says exactly that in its own comment, and then ran
// a fast path for three names — `call` / `apply` / `bind` — ahead of
// it. A handler's `get` trap never saw those reads, and a handler
// that ANSWERED them was ignored outright: `p.call` overridden to
// return a different function still ran the target.
//
// The fast path itself is right, and needed: `p.call(t, …)` is an
// operation on the PROXY (§20.2.3.3 passes its own receiver through
// as the thisArgument), so forwarding to the target would skip the
// `apply` trap. It is only exact when there is no `get` trap — then
// the read is unobservable and cannot answer anything but the
// intrinsic. With a trap, the read runs, and the intrinsic it hands
// back re-enters the same forwarder through the builtin dispatch,
// `this` still the proxy.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
function dbl(x: number): number {
  return x * 2;
}
const watch = (seen: string[]): any => ({
  get(t: any, k: any) {
    seen.push(String(k));
    return Reflect.get(t, k);
  },
});
const override = (name: string): any => ({
  get: (t: any, k: any) => (k === name ? () => "OVERRIDE" : Reflect.get(t, k)),
});

for (const n of ["call", "apply", "bind"]) {
  const seen: string[] = [];
  const p: any = new Proxy(dbl as any, watch(seen));
  attempt(() => (n === "apply" ? p.apply(null, [1]) : n === "call" ? p.call(null, 1) : p.bind(null)));
  console.log("trap sees " + n.padEnd(6), attempt(() => seen.join(",")));
  console.log("override  " + n.padEnd(6), attempt(() => new Proxy(dbl as any, override(n))[n](null)));
}
// With BOTH traps the read runs and then the apply trap does.
const both: any = new Proxy(dbl as any, {
  get: (t: any, k: any) => Reflect.get(t, k),
  apply: (_t: any, _th: any, a: any) => a[0] * 7,
});
console.log("get+apply call  ", attempt(() => both.call(null, 3)));
console.log("get+apply apply ", attempt(() => both.apply(null, [3])));
console.log("get+apply bind  ", attempt(() => both.bind(null, 3)()));
// No `get` trap: the short-circuit stands, traps and all.
const trapless: any = new Proxy(dbl as any, {});
const applyOnly: any = new Proxy(dbl as any, {
  apply: (_t: any, _th: any, a: any) => a[0] * 3,
});
console.log("trapless call   ", attempt(() => trapless.call(null, 4)));
console.log("trapless apply  ", attempt(() => trapless.apply(null, [4])));
console.log("applyOnly call  ", attempt(() => applyOnly.call(null, 5)));
console.log("applyOnly bind  ", attempt(() => applyOnly.bind(null, 5)()));
// A revoked proxy throws on the [[Get]] itself, before any of this.
console.log("revoked call    ", attempt(() => {
  const r: any = Proxy.revocable(dbl as any, {});
  r.revoke();
  return r.proxy.call(null, 1);
}));
// §7.3.2 step 3's Call asks §7.2.3 of whatever the trap answered, so
// a class object is callable-and-refused, and another proxy runs.
console.log("trap gives class", attempt(() => {
  class C {}
  return (new Proxy({} as any, { get: () => C }) as any).anything();
}));
console.log("trap gives proxy", attempt(() =>
  (new Proxy({} as any, { get: () => new Proxy((() => 42) as any, {}) }) as any).anything()));
console.log("trap gives 5    ", attempt(() =>
  (new Proxy({} as any, { get: () => 5 }) as any).anything()));
// Ordinary functions never touched any of this.
console.log("plain fn        ", attempt(() =>
  dbl.call(null, 1) + "/" + dbl.apply(null, [2]) + "/" + dbl.bind(null, 3)()));
