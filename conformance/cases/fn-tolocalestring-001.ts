// L3b 5 residue -- Function.prototype.toLocaleString answers
// toString's type-erased source (ES 20.2.3.5), on both the static
// fn-typed lane and the any lane; namespace statics fold the named
// native form.
function f(a: number): number {
  return a + 1;
}
const fa: any = f;
console.log(fa.toLocaleString());
console.log(f.toLocaleString());
const g = (x: number) => x * 2;
console.log(g.toLocaleString());
console.log(Math.max.toLocaleString());
console.log(f.toLocaleString() === f.toString());
