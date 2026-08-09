// RFC 20260810 knife 1 — §15.5.5 the self-name BINDS inside the body
// (a trailing self-slot on the env, borrowed self-edge): typeof,
// recursion, nested-arrow read, param shadow, capture+self-slot
// coexistence, and the immutable-binding write (strict TypeError
// through the readonly-assign kernel; bun words it identically).
var f: any = function me() {
  return typeof me;
};
console.log(f());
var fact: any = function rec(n: number): number {
  return n <= 1 ? 1 : n * rec(n - 1);
};
console.log(fact(5));
var g: any = function outer() {
  return (() => typeof outer)();
};
console.log(g());
var shadow: any = function s(s: number) {
  return typeof s;
};
console.log(shadow(1));
let base = 10;
var withCap: any = function add(n: number): number {
  return n <= 0 ? base : n + add(n - 1);
};
console.log(withCap(3));
var w: any = function wr() {
  wr = 1 as any;
  return wr;
};
try {
  w();
} catch (e) {
  console.log((e as any).name, (e as any).message);
}
var wa: any = function wrArrow() {
  return (() => {
    wrArrow = 2 as any;
  })();
};
try {
  wa();
} catch (e) {
  console.log((e as any).name, (e as any).message);
}
