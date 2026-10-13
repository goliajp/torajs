// A fn-typed binding or parameter handed on to another slot of the same
// annotation holds the value it holds, of another face than both
// annotations spell: `q` returns what its `any` callback answers, so a
// call through every slot it reaches answers that value as is.
type T = (x: number, f: () => any) => number;
function q(x: number, f: () => any): number { return f() }
function run2(g: T): number { return g(2, () => "t") }
function run(g: T): number { return run2(g) }
console.log(run(q));
const h: T = q;
const k: T = h;
console.log(k(1, () => "u"));
function viaLet(g: T): number {
  const j: T = g;
  return j(3, () => "a");
}
console.log(viaLet(q));
let s: T = (x: number, f: () => any): number => x;
console.log(s(4, () => "no"));
s = h;
console.log(s(5, () => "d"));
const obj = { m(g: T) { return g(6, () => "e") } };
function relay(g: T) { return obj.m(g) }
console.log(relay(q), obj.m((x: number) => x + 1));
