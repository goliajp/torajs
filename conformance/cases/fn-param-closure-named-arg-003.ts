// What reaches a closure-typed parameter from the calling frame takes
// the closure form too: a class method whose parameter also receives
// arrows, given a forwarded parameter, a choice among named functions
// and a function's returned function; and a let-bound arrow stored in
// an object literal, given the same.
class R {
  run(f: (v: number) => void) {
    f(5);
  }
}
const r = new R();
r.run((v: number) => console.log("arrow", v));
function named(v: number) {
  console.log("named", v);
}
function other(v: number) {
  console.log("other", v);
}
function g(cb: (v: number) => void) {
  r.run(cb);
}
g(named);
for (const c of [true, false]) r.run(c ? named : other);
function make(): (v: number) => void {
  return other;
}
r.run(make());
let t = (f: (v: number) => void) => f(1);
const o = { k: t };
o.k((v: number) => console.log("let arrow", v));
function h(cb: (v: number) => void) {
  o.k(cb);
}
h(named);
function make2(): (v: number) => void {
  return named;
}
o.k(make2());
