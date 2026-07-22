// resolve() with a still-pending promise: the withResolvers promise
// stays pending and adopts the inner's eventual state (§27.2.1.3.2).
const b: any = Promise.withResolvers();
b.resolve(Promise.reject("boom").catch((e: any) => "recovered:" + e));
b.promise.then((v: any) => console.log("adopt-pending-chain:", v));
