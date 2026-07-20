// L3b combinator residual face ② — combinator results settle one
// microtask AFTER mint (the spec resolve-absorption round), so their
// .then callbacks interleave with direct-mint chains exactly as bun
// orders them (the sync-settle fast path fired them one round
// early). Mixed sources are the observable: the direct-mint cb must
// print before the combinator cb even though the combinator line
// comes first.
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then(function (r: any) {
  console.log("all", r.length);
});
Promise.resolve(9).then(function (v: number) {
  console.log("direct", v);
});
Promise.any([Promise.reject(3), Promise.resolve(4)]).then(function (v: any) {
  console.log("any", v);
});
Promise.resolve(8).then(function (v: number) {
  console.log("direct2", v);
});
