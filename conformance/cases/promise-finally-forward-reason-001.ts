// `.finally` settles its promise with the source's own value or reason
// when the handler returns normally (§27.2.5.3 thenFinally /
// catchFinally). The source promise is let go before the value is
// read, so the forwarded value has to be kept alive on its own.
process.on("unhandledRejection", (reason: any) => {
  console.log("unhandled", reason instanceof TypeError, reason.message);
});
Promise.reject(new TypeError("a")).finally(() => {});
const caught = Promise.reject(new TypeError("b")).finally(() => console.log("finally b"));
const kept = Promise.resolve({ n: 1, s: "text" }).finally(() => console.log("finally kept"));
Promise.resolve()
  .then(() => 0)
  .then(() => 0)
  .then(() => {
    caught.catch((e: TypeError) => console.log("caught", e instanceof TypeError, e.message));
    kept.then((v) => console.log("kept", v.n, v.s));
  });
Promise.resolve([1, 2, 3])
  .finally(() => {})
  .then((xs) => console.log("array", xs.length, xs[2]));
