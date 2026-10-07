// A function that reads `arguments` and is called through an alias typed
// `any` takes calls of any width: the argument list belongs to the call,
// not to the declared parameters, and a missing argument is `undefined`
// in the parameter as well as in the count.
function h(a: number) {
  return arguments.length + ":" + a;
}
const hv: any = h;
console.log(hv(), hv(1), hv(1, 2), hv(undefined), hv("s"), hv(1.5));
console.log(h(4), hv(7));

function n(a: number) {
  return arguments.length + ":" + (a === undefined) + ":" + typeof a + ":" + (a + 1);
}
const nv: any = n;
console.log(nv(), nv(2), nv(2, 3));

function k(a: number) {
  return arguments.length;
}
const kv: any = k;
console.log(kv(), kv(4, 5));

function hs(a: string, b: boolean) {
  return arguments.length + ":" + a + ":" + b;
}
const sv: any = hs;
console.log(sv(), sv("x"), sv("x", true, 3), sv(1, 0));

type P = { x: number };
function hp(p: P) {
  return arguments.length + ":" + (p === undefined ? "u" : p.x);
}
const pv: any = hp;
console.log(pv(), pv({ x: 3 }), pv({ x: 4 }, 1));

function ha(xs: number[]) {
  return arguments.length + ":" + (xs === undefined ? "u" : xs.length);
}
const av: any = ha;
console.log(av(), av([1, 2]), av([1], 2, 3));

function hr(a: number, ...xs: number[]) {
  return arguments.length + ":" + a + ":" + xs.length;
}
const rv: any = hr;
console.log(rv(), rv(1), rv(1, 2, 3));

function hu(a, b) {
  return arguments.length + ":" + a + ":" + b;
}
const uv: any = hu;
console.log(uv(), uv(1), uv(1, 2, 3));

function q(a: any) {
  return arguments[1] + ":" + a + ":" + arguments.length;
}
const qv: any = q;
console.log(qv(), qv(3), qv(3, 4));

function hc(a: number) {
  return arguments.length + ":" + a;
}
const cv: any = hc;
console.log(cv.call(null), cv.apply(null, [1, 2]), [1, 2].map(cv).join(","));
