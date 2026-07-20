// RFC 20260720-promise-any-cb residual — (v: any) => R handlers over
// the Array-inner then/catch lane (P10.2-A4 arm), the combinator-
// result shape. No rejected inputs here — the absorbed-input face is
// its own fixture.
Promise.all([Promise.resolve(1), Promise.resolve(2)]).then(function (r: any) {
  console.log("all-len", r.length);
});
Promise.all([Promise.resolve(3), Promise.resolve(4)]).then((r: any) => {
  console.log("all-first", r[0]);
});
