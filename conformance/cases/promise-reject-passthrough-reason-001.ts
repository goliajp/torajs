// A rejection passing through `.then(onFulfilled)` with no onRejected
// settles the derived promise with the same reason (§27.2.5.4.1
// PerformPromiseThen, §27.2.1.3.2 the reaction's Thrower), whatever
// value the handler that never ran would have produced. The derived
// promise is the one left unhandled here, and its reason must still be
// the TypeError, also after the source promise is gone.
process.on("unhandledRejection", (reason: any) => {
  console.log("unhandled", reason instanceof TypeError, reason.message);
});
Promise.reject(new TypeError("x")).then(console.log);
Promise.reject(new TypeError("y")).then((v: number) => "s" + v);
const thenable = {
  then(res: (v: number) => void) {
    throw new TypeError("z");
  },
};
Promise.resolve(thenable).then((v) => console.log("never", v));
