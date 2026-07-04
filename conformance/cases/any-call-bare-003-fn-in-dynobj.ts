// any-method-call RFC C4+ — named top-level fns inside an any
// ObjectLit / Array init wrap in their forwarder closures, so dynobj
// fields and any-array slots hold callable cells.
function add(a: number, b: number): number {
  return a + b;
}
function mk(): any {
  const inner = (x: number) => x + 10;
  return inner;
}
const o: any = { plus: add, getFn: mk };
console.log(o.plus(2, 3));
// call-returns-any as callee dispatches through the bare route
console.log(o.getFn()(5));
// member read then bare call
const f: any = o.plus;
console.log(f(7, 8));
// array element position
const arr: any = [add];
console.log(arr[0](20, 22));
console.log("done");
