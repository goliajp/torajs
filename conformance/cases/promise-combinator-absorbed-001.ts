// Combinator inputs are HANDLED by the combinator itself (the spec's
// per-input resolve/reject-element attach, §27.2.4) — a REJECTED
// input a combinator absorbs must not fire the unhandled-rejection
// reporter (was: trailing "error: 1" + exit 1 where bun exits 0).
// Every source here is a combinator product so the known sync-settle
// microtask-round offset (L3b residual face ②) has no observable
// interleaving: relative cb order follows attach order in both
// runtimes.
Promise.any([Promise.reject(1), Promise.resolve(2)]).then(function (v: any) {
  console.log("any", v);
});
Promise.race([Promise.reject(3), Promise.resolve(4)]).catch(function (e: any) {
  console.log("race-caught", e);
});
Promise.allSettled([Promise.reject(5), Promise.resolve(6)]).then(function (
  r: any
) {
  console.log("allSettled", r.length);
});
Promise.all([Promise.reject(7), Promise.resolve(8)]).catch(function (e: any) {
  console.log("all-caught", e);
});
