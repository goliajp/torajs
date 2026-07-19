// RFC 20260719-fn-tostring-source B4a — Function.prototype.toString
// answers the type-erased source text through the any lane; rows
// with no recorded source fall to the JSC native form.
function add(a: number, b: number): number {
  return a + b;
}
const fa: any = add;
console.log(fa.toString());
const arrow: any = (x: number) => x * 2;
console.log(arrow.toString());
const multi: any = (a: number, b: string): string => {
  return b + a;
};
console.log(multi.toString());
const m: any = fa.toString;
console.log(m.call(fa));
const bound: any = add.bind(null);
console.log(bound.toString());
const s: any = "hi";
const reified: any = s.toUpperCase;
console.log(reified.toString());
console.log(add(1, 2));
