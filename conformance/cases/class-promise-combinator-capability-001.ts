// §27.2.4.1 step 2 on the inherited combinators — the result rides
// NewPromiseCapability(C), so `CP.all(...)` answers a C INSTANCE
// that settles with the combinator's outcome. The element walk
// still rides the builtin kernel (per-element GetPromiseResolve(C)
// is the next layer), so cross-chain tick ordering is not asserted
// here — one chain per observation.
class CP extends Promise<any> {}
const cp: any = CP;
const pa = cp.all([1, Promise.resolve(2)]);
console.log(pa instanceof CP);
pa.then((xs: any) => console.log("all", xs[0] + xs[1]));
