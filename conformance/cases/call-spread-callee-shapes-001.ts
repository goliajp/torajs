// §13.3.8.1 — a spread in the argument list changes how the list is
// built, not what may be called. `f(x)` and `f(...[x])` ask the same
// §7.2.3 question about `f`, and the answer has to be the same.
//
// The bare-call spread kernel re-implemented the dispatch tail with
// the closure arm alone, so it refused two of the three callable
// shapes its fixed-argc twin accepts: a callable Proxy, and a class
// object. `p(1)` ran and `p(...[1])` threw, on the very same value.
// The construct and super-method spread kernels never had the bug —
// both call their twin instead of restating it.
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
const pfn: any = new Proxy(dbl as any, {});
const ptrap: any = new Proxy(dbl as any, {
  apply: (_t: any, _th: any, a: any) => a[0] * 10,
});
const args: any = [4];
class K {}
const pcls: any = new Proxy(K as any, {});

console.log("fixed  proxy   ", attempt(() => pfn(4)));
console.log("spread proxy   ", attempt(() => pfn(...args)));
console.log("fixed  trap    ", attempt(() => ptrap(4)));
console.log("spread trap    ", attempt(() => ptrap(...args)));
console.log("spread mixed   ", attempt(() => pfn(...[], ...args)));
// A class object is callable and throws when called — the same
// TypeError either way, not "not a function" from one spelling.
console.log("fixed  class   ", attempt(() => (K as any)(4)));
console.log("spread class   ", attempt(() => (K as any)(...args)));
console.log("spread pcls    ", attempt(() => pcls(...args)));
// Genuinely not callable, both spellings.
console.log("fixed  object  ", attempt(() => ({} as any)(4)));
console.log("spread object  ", attempt(() => ({} as any)(...args)));
console.log("spread of obj p", attempt(() => (new Proxy({} as any, {}) as any)(...args)));
// Plain closures unmoved, including the over-8 argument path.
console.log("spread closure ", attempt(() => dbl(...args)));
console.log("spread many    ", attempt(() =>
  ((...xs: number[]) => xs.length)(...([1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as any))));
