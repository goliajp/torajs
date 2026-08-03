// Function.prototype call/apply/bind nullary + nullish-argArray forms
// (rotation 291): `f.call()` / `f.apply()` / `f.apply(t)` /
// `f.apply(t, undefined|null)` are all the bare invocation per
// §22.2.3.1; a partial list covering every param leaves a zero-param
// bound fn. Also the closure-interior `f.apply()` shape that used to
// reject whole-program (box_to_any FnSig via the capture lane).

function zero() {
  return "zero-ran";
}

function two(a: number, b: number) {
  return a + b;
}

// call: empty form
console.log(zero.call());
// apply: absent / thisArg-only / explicit nullish argArray
console.log(zero.apply());
console.log(zero.apply(undefined));
console.log(zero.apply(null));
console.log(zero.apply(undefined, undefined));
console.log(zero.apply(null, null));
// apply: array-literal unpack still works
console.log(two.apply(null, [3, 4]));
// bind: thisArg-only on a zero-param fn
const b0 = zero.bind(null);
console.log(b0());
// bind: full partial list -> zero-param bound fn
const b2 = two.bind(null, 10, 20);
console.log(b2());
// closure-interior apply (the sweep-cluster shape)
function runner(cb: any) {
  cb();
}
runner(function () {
  console.log(zero.apply());
});
