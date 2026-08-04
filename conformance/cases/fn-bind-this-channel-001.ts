// RFC 20260804-fn-this-channel knife 4 — .bind forwards its thisArg.
// bind_this_param made the receiver param 0, so binding a thisArg is
// just partially applying that slot: it leads the partial list and
// rides the factory's capture env like any other bound argument.
function f(this: any, a: number) {
  return (this as any)._v + a;
}
console.log(f.bind({ _v: 10 })(5));
console.log(f.bind({ _v: 100 }, 7)());

function g(this: any) {
  return (this as any)._v;
}
console.log(g.bind({ _v: 3 })());

// the bound fn is a value: it survives being passed around
const bound = f.bind({ _v: 20 });
function apply1(cb: any, n: number) {
  return cb(n);
}
console.log(apply1(bound, 2));

// a this-free target keeps the historic thisArg drop — no receiver
// slot exists to bind, and its body never reads one
function h(a: number, c: number) {
  return a + c;
}
console.log(h.bind(null, 1)(2));
console.log(h.bind(null)(4, 5));
