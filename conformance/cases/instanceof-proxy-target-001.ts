// §7.3.22 OrdinaryHasInstance over a Proxy right-hand side. Step 1 is
// IsCallable, which §10.5.14 step 7 fixes from the proxy's target;
// steps 4-7 are ordinary, with the `get` trap running on "prototype".
// tr's walk knew two shapes — a closure cell and a class object — so
// EVERY proxy answered "Right-hand side of 'instanceof' is not
// callable", including a proxy over a plain function.
function attempt(f: any): string {
  try {
    return String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
class K {
  m(): number {
    return 1;
  }
}
const Kc: any = K;
const k: any = new K();
console.log("proxy over class   ", attempt(() => k instanceof new Proxy(Kc, {})));

function F(): void {}
const pf: any = new Proxy(F as any, {});
const inst: any = Object.create(pf.prototype);
console.log("proxy over fn      ", attempt(() => inst instanceof pf));

const pE: any = new Proxy(Error as any, {});
console.log("proxy over Error   ", attempt(() => new Error("m") instanceof pE));
console.log("negative           ", attempt(() => ({}) instanceof pE));

// A `get` trap is an ordinary Get, so it is the one consulted.
const trapped: any = new Proxy(F as any, {
  get(t: any, k: any, r: any) {
    return k === "prototype" ? K.prototype : Reflect.get(t, k, r);
  },
});
console.log("get trap decides   ", attempt(() => k instanceof trapped));

// A proxy over a NON-callable target is still step 1's refusal.
const po: any = new Proxy({} as any, {});
console.log("proxy over object  ", attempt(() => k instanceof po));
