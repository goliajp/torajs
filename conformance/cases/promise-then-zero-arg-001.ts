// §27.2.5.4 with both handlers absent — `p.then()` / `p.catch()`:
// onFulfilled defaults to Identity and onRejected to Thrower, so
// the derived promise settles exactly as the source does, one
// reaction tick later (t262 rxn-handler-identity / -thrower use
// the bare spelling as chain spacers).
const obj: any = {};
Promise.resolve(obj)
  .then()
  .then((arg: any) => {
    console.log("identity", arg === obj);
  });
Promise.resolve(1)
  .then()
  .then()
  .then((v: any) => console.log("chain", v));
Promise.reject(new Error("boom"))
  .then()
  .catch((e: any) => console.log("thrower", e.message));
Promise.resolve(2)
  .catch()
  .then((v: any) => console.log("catch0", v));
