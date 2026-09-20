// §27.2.1.5 step 8/9 and §27.2.4.1.1 step 2 both ask §7.2.3
// IsCallable. The promise lane asked something slightly different —
// "a closure cell with a boxed dual entry, exactly what Call would
// accept" — and the two diverge on a class object, which Call does
// accept and then refuses on its own terms (§10.2.1 step 2).
//
// The spec puts IsCallable at the gate and the throw inside the Call,
// so a gate that pre-empts it answers in the wrong place.
class K {
  m(): number {
    return 1;
  }
}
const Kc: any = K;

async function main(): Promise<void> {
  // §27.2.1.3.2 — a thenable whose `then` is callable: the job runs
  // Call, the class refuses, the promise rejects. It used to be
  // "not thenable", so the promise FULFILLED with the object.
  const th: any = { then: Kc };
  try {
    const v: any = await Promise.resolve(th);
    console.log("thenable fulfilled", String(v));
  } catch (e: any) {
    console.log("thenable rejected", e && e.constructor ? e.constructor.name : "?");
  }

  // §27.2.4.1.1 step 2 — GetPromiseResolve(C) over an EMPTY iterable
  // has nothing to call, so the result fulfills with []. The gate
  // used to reject it before looking.
  class MyP extends Promise<any> {}
  const MP: any = MyP;
  MP.resolve = Kc;
  try {
    const v: any = await MP.all([]);
    console.log("all([]) fulfilled", JSON.stringify(v));
  } catch (e: any) {
    console.log("all([]) rejected", e && e.constructor ? e.constructor.name : "?");
  }

  // §26.2.1.1 step 2 — a class is a valid cleanupCallback; the spec
  // does not refuse at construction.
  try {
    const f: any = new FinalizationRegistry(Kc);
    console.log("finreg constructed", typeof f);
  } catch (e: any) {
    console.log("finreg threw", e && e.constructor ? e.constructor.name : "?");
  }
}
main();
