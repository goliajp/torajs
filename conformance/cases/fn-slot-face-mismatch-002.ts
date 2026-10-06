// a fn-typed slot declaring a typed return over a function literal whose
// parameter types differ: a call whose argument does not fit the literal
// answers what the literal's body computes from it, not that value
// converted to the declared return
const h: (s: any) => number = (s: string) => s.length;
console.log(h("abc"), h(5));
const two: (a: any, b: any) => string = (a: string, b: number) => a + ":" + b;
console.log(two("x", 1), two(1, "y"));
function apply(f: (s: any) => number, v: any): any {
  return f(v);
}
console.log(apply((s: string) => s.length, 5));
const k: number = h("zz");
console.log(k);
let m: (s: any) => number = (s: string) => s.length;
console.log(m(true), m("four") + 1);
// `.call` / `.apply` replay the same call
console.log(h.call(null, 5), h.apply(null, [7]), h.call(undefined, "xy") + 1);
console.log(h.apply(null, ["abcd"]) * 2);
// a slot declaring an object return
const o: (s: any) => { n: number } = (s: string) => ({ n: s.length });
console.log(o("abc").n, o(5).n);
// called from inside another function
function viaTop(v: any): any {
  return h(v);
}
console.log(viaTop(9), viaTop("q"));
// same-named bindings of the exact face keep their number answer
// through arithmetic
function inner(h: (s: number) => number): number {
  return h(2) * 10 + 1;
}
console.log(inner((x: number) => x + 1));
{
  const h: (s: string) => number = (s: string) => s.length * 2;
  console.log(h("abc") + 1);
}
function other(f: (s: any) => number): number {
  return f(2) * 2;
}
console.log(other((x: any) => 20) + 1);
// the slot's return survives when the body's answer does not depend
// on the argument's type
const c: (s: any) => number = (s: string) => 7;
console.log(c(5) + 1);
