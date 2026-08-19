// §27.2.4.1.1 GetPromiseResolve(%Promise%) — one REAL Get per
// combinator run, before iteration: an accessor-patched
// `Promise.resolve` getter observes exactly one read per run (t262
// invoke-resolve-get-once family), the fetched function then runs
// once per element, and a getter throw rejects BEFORE the iterable
// is touched (invoke-resolve-get-error asserts that order).
let getCount = 0;
let callCount = 0;
const bound = Promise.resolve.bind(Promise);
Object.defineProperty(Promise, "resolve", {
  configurable: true,
  get() {
    getCount += 1;
    return function (...args: any[]) {
      callCount += 1;
      return bound(...args);
    };
  },
});
async function main() {
  await Promise.all([1, 2, 3]);
  console.log("get", getCount, "call", callCount);
  await Promise.all([]);
  console.log("get", getCount, "call", callCount);
  const boom = { name: "MyError" };
  Object.defineProperty(Promise, "resolve", {
    configurable: true,
    get() {
      throw boom;
    },
  });
  const iter = {
    get [Symbol.iterator](): any {
      console.log("iterator observed");
      throw new Error("unreachable");
    },
  };
  try {
    await Promise.all(iter as any);
    console.log("not rejected");
  } catch (e: any) {
    console.log("rejected", e === boom);
  }
}
main();
