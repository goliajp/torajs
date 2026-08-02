// §23.1.3.11 step 8.d — a non-Array callback result is pushed as-if
// `[U]`; a value-less callback's result is `undefined`, so the product
// is an array of undefineds (bun: [undefined, undefined, undefined]).
const xs = [1, 2, 3];
const r = xs.flatMap((n) => {
  console.log(n);
});
console.log(r.length, r[0]);
const r2 = xs.flatMap((n) => undefined);
console.log(r2.length, r2[1]);
