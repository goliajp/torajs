// `(f as any)[0] = 3` on a function value died in lowering ("index
// assign on non-array Closure"): the `as any` is a bare pass-through
// for a heap source, so the receiver reached the index-assign lane as
// a Closure operand under an `any` recorded type, and the lane's
// box-to-any tests looked only at the recorded type (a `Function`
// spelling) — the typed `g[0] = 3` boxed, the cast one did not. The
// operand's own shape decides now, the way an expando-bag receiver
// already did: the closure cell's numeric write is the §7.1.19
// stringified expando write the any index-set kernel owns.
const f = function (a: any, b: any) {};
(f as any).x = 1;
(f as any)["y"] = 2;
(f as any)[0] = 3;
console.log("cast", (f as any).x, (f as any)["y"], (f as any)[0]);
const g = function (a: any, b: any) {};
g[0] = 4;
g[1] = 5;
console.log("typed", g[0], g[1], g.length);
async function viaAsync() {
  const fn = function (a: any, b: any) {};
  (fn as any)[0] = 6;
  (fn as any)[1] = 7;
  await Promise.resolve(0);
  return (fn as any)[0] + (fn as any)[1];
}
viaAsync().then((v: any) => console.log("async", v));
