// §27.2.1.5 NewPromiseCapability(C) behind the inherited settle
// statics: `CP.resolve(v)` on `class CP extends Promise` constructs
// a REAL C instance through the runtime construct channel (the
// subclass ctor chain observably runs), then settles it through the
// recorded resolving pair. Recorded boundaries: PromiseResolve's
// step-2 identity fast path is not taken, and `.then`-derived
// promises ride the builtin species (SpeciesConstructor §27.2.5.4
// is the next layer) — so no ctor-count or identity-of-argument
// assertions here.
class CP extends Promise<any> {}
const cp: any = CP;
const p = cp.resolve(41);
console.log(p instanceof CP, p instanceof Promise);
p.then((v: any) => console.log("resolved", v + 1));
cp.reject(new Error("boom")).catch((e: any) => console.log("caught", e.message));

class Tracked extends Promise<any> {
  constructor(ex: any) {
    super(ex);
    console.log("ctor ran");
  }
}
const tp = (Tracked as any).resolve(7);
console.log(tp instanceof Tracked);
