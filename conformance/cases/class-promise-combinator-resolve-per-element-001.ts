// §27.2.4.1.2 PerformPromiseAll step 1 GetPromiseResolve(C) +
// step 6.i Call(promiseResolve, C, «v») per element — a patched
// `C.resolve` on a builtin-heir class observes every iteration
// (t262 invoke-resolve-*-every-iteration-of-custom), and the
// builtin `Promise.resolve` patch lane stays silent for the whole
// run. One chain of observations across all four combinators.
class Custom extends Promise<any> {}
const cust: any = Custom;
const prom: any = Promise;
let cCount = 0;
let pCount = 0;
const boundCustomResolve = cust.resolve.bind(Custom);
const boundPromiseResolve = prom.resolve.bind(Promise);
cust.resolve = function (...args: any[]) {
  cCount += 1;
  return boundCustomResolve(...args);
};
prom.resolve = function (...args: any[]) {
  pCount += 1;
  return boundPromiseResolve(...args);
};
prom.all
  .call(Custom, [1, 2, 3])
  .then((r: any) => {
    console.log("all", cCount, pCount, r.join(","));
    return prom.race.call(Custom, [4, 5]);
  })
  .then((r: any) => {
    console.log("race", cCount, pCount, r);
    return prom.allSettled.call(Custom, [6]);
  })
  .then((r: any) => {
    console.log("allSettled", cCount, pCount, r[0].status, r[0].value);
    return prom.any.call(Custom, [7]);
  })
  .then((r: any) => {
    console.log("any", cCount, pCount, r);
  });
