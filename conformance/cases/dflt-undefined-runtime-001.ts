// S3.8 — a runtime `undefined` arriving in an `any`-typed actual
// binds a typed param's Number/Bool literal default (§10.2.11), on
// the direct named-fn call lane: the or_default kernel substitutes
// ahead of the Any→typed coercion (which used to answer NaN/false).
// A non-undefined any value passes through untouched.
const u: any = undefined;
const v: any = 7;
function f(x: number = 5): number {
  return x;
}
console.log(f(u));
console.log(f(v));
function h(x: number = 2.5): number {
  return x;
}
console.log(h(u));
function b(a: number, flag: boolean = true): boolean {
  return flag;
}
console.log(b(1, u));
console.log(b(1, false));
function m(a: number = 1, c: number = 0): number {
  return a + c;
}
console.log(m(u, 7));
