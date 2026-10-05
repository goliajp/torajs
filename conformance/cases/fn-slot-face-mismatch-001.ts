// a function literal stored into a fn-typed binding whose annotation
// declares a parameter type other than the literal's (legal: `any` is
// assignable to `string`): calls through the binding pass whatever the
// annotation admits, and the literal's body reads it under its own
// parameter types only when the argument fits
const h: (s: any) => any = (s: string) => s.length;
console.log(h("abc"), h(5), h([1, 2]));
let m: (s: any) => any = (s: string) => s.toUpperCase();
console.log(m("ab"));
try {
  m(7);
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}
const two: (a: any, b: any) => any = (a: string, b: number) => a + ":" + b;
console.log(two("x", 1), two(1, "y"));
// a same-face binding keeps its direct call
const same: (s: string) => number = (s: string) => s.length;
console.log(same("four"));
