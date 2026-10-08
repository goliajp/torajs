// A declaration with a fn-typed parameter, called through `any` with a
// named function, an arrow, and a value that is no function at all
// (§10.2.1.3 binds each argument as it is; calling the non-function
// throws a TypeError in the body).
function nf(v: number) {
  console.log("nf", v);
}
function take(f: (v: number) => void) {
  f(3);
}
function both(f: (v: number) => void, n: number) {
  f(n * 2);
}
const t: any = take;
t(nf);
t((v: number) => console.log("arrow", v));
const b: any = both;
b(nf, 5);
try {
  b(3, 1);
} catch (e) {
  console.log("not callable", e instanceof TypeError);
}
b((v: number) => console.log("both", v), "x");
take(nf);
take((v: number) => console.log("typed", v));
