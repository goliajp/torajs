// A function declared to return a scalar hands back, as it is, a value
// of another type that reaches its return through `any` (§10.2.1.12):
// a binding annotated `any` holding a string, a cast of a string, of a
// choice between a string and a number, of a comparison's boolean, of
// an optional parameter that may be undefined, and an arithmetic
// result over a bigint.
function f(): number {
  const s: any = "x";
  return s;
}
console.log(typeof f() + ":" + f());
function g(x: string): number { return x as any }
console.log(typeof g("y") + ":" + g("y"));
function h(b: boolean): number { return (b ? "s" : 1) as any }
console.log(h(true), h(false));
function c(a: number, b: number): number { return (a < b) as any }
console.log(typeof c(1, 2) + ":" + c(1, 2));
function w(x: number): string { return x as any }
console.log(typeof w(3) + ":" + w(3));
function n(x?: number): number { return x as any }
console.log(typeof n() + ":" + n(), n(2));
function neg(x: any): number { return -x }
console.log(typeof neg(2n) + ":" + neg(2n), neg(4));
function v(b: boolean) {
  if (b) return 1;
  const s: any = "x";
  return s;
}
console.log(typeof v(false) + ":" + v(false), v(true));

// a value of the declared type converts nothing
function same(): number {
  const k: any = 41;
  return k;
}
console.log(same() + 1);
