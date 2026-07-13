// RFC 20260713-generator-fn-value-substrate blade 5 cut 4 —
// %GeneratorFunction% / %AsyncGeneratorFunction% intrinsic chain:
// getPrototypeOf(generator fn) answers the shared fn-proto
// singleton, its .constructor/.prototype wire per §27.3, every
// per-generator .prototype chains to %GeneratorPrototype%, and the
// __proto__ simulation key stays hidden from own-keys surfaces.
// Ordinary fns answer %Function.prototype%.

function* g() {
  yield 1;
}
async function* ag() {
  yield 2;
}
function f() {
  return 3;
}

const gfp: any = Object.getPrototypeOf(g);
console.log("gfp typeof:", typeof gfp);
console.log("GF name:", gfp.constructor.name);
console.log("GF length:", gfp.constructor.length);
console.log("ctor.prototype loops:", gfp.constructor.prototype === gfp);
console.log("same for genexpr:", gfp === Object.getPrototypeOf(function* () {}));
console.log("chain:", gfp.prototype === Object.getPrototypeOf(g.prototype));

const agfp: any = Object.getPrototypeOf(ag);
console.log("AGF name:", agfp.constructor.name);
console.log("distinct intrinsics:", gfp !== agfp);

console.log("own props empty:", Object.getOwnPropertyNames(g.prototype).length);
console.log(
  "fn proto is Function.prototype:",
  Object.getPrototypeOf(f) === Object.getPrototypeOf(function () {})
);
console.log("relation:", Object.getPrototypeOf(gfp) === Object.getPrototypeOf(f));
