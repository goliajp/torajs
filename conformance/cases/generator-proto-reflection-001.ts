// RFC 20260721-builtin-method-reflection 刀 2 — the generator fn
// proto chain: Object.getPrototypeOf(g) answers the shared
// %GeneratorFunction.prototype% (genfn trio), whose `.prototype` is
// %GeneratorPrototype% carrying the next/return/throw reflection
// cells (name / length per §27.5.1.2-4).
function* g() {
  yield 1;
}
const gAny: any = g;
const gfp: any = Object.getPrototypeOf(gAny);
console.log(typeof gfp);
const GP: any = gfp.prototype;
console.log(typeof GP);
console.log(typeof GP.next, typeof GP.return, typeof GP.throw);
console.log(GP.next.name, GP.next.length);
console.log(GP.return.name, GP.return.length);
console.log(GP.throw.name, GP.throw.length);
function* g2() {
  yield 2;
}
const g2Any: any = g2;
const gfp2: any = Object.getPrototypeOf(g2Any);
if (gfp === gfp2) {
  console.log("genfnproto-shared");
} else {
  console.log("genfnproto-BAD");
}
console.log(gfp.constructor.name);
