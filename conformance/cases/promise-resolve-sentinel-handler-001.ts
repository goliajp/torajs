// `Promise.resolve(x)` settles with `x` and §27.2.5.4 hands it to the
// fulfilled handler untouched. A sentinel-bearing `x` read through a
// BINDING answered `typeof v` "number" inside the handler, while the
// same value written inline at the call answered "undefined" — the
// taint stopped at the binding because nothing tracked it across
// bodies.
const zs: number[] = [];

Promise.resolve(zs[9]).then((v) => {
  console.log("inline", typeof v, v === undefined);
});

const u: number = zs[9];
Promise.resolve(u).then((v) => {
  console.log("binding", typeof v, v === undefined);
});

let w: number = 1;
w = zs[8];
Promise.resolve(w).then((v) => {
  console.log("assigned", typeof v, v === undefined);
});

// A live value keeps the plain number lane inside the handler.
const live: number = 41;
Promise.resolve(live).then((v) => {
  console.log("live", typeof v, v + 1);
});
