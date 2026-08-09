// allSettled records read back through the any lane — r347 recorded
// this as "record not readable" (r[0].status answered undefined);
// the rotation-348 owned-contract fix restored it. Locked here.
Promise.allSettled([Promise.resolve(3)]).then(function (r: any) {
  console.log(r[0].status, r[0].value);
});
Promise.allSettled([Promise.reject(7)]).then(function (r: any) {
  console.log(r[0].status, r[0].reason);
});
Promise.allSettled([1, Promise.resolve("x")]).then(function (r: any) {
  console.log(r.length, r[0].status, r[0].value, r[1].status, r[1].value);
});
