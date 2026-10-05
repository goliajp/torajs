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
// the literal reaches the binding through another binding
const k = (s: string) => s.length;
const viaK: (s: any) => any = k;
console.log(viaK("abcd"), viaK(null === null ? 3 : 4), k("xy"));
// a fn-typed parameter filled with such a literal
function apply(f: (s: any) => any, v: any): any {
  return f(v);
}
console.log(apply((s: string) => s.length, "abc"), apply((s: string) => s.length, 5));
console.log(apply(k, "hello"), apply((n: number) => n + 1, 1));
// a named function stored into a binding whose annotation widens a
// parameter
function k2(s: string) {
  return s.length;
}
const viaNamed: (s: any) => any = k2;
console.log(viaNamed("abcd"), viaNamed(5), k2("z"));
