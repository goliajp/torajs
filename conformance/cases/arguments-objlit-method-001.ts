// RFC 20260801 objlit branch — an object-literal method lifts to a
// closure stored in the ObjectLit field; its member call sites are
// invisible to the named-fn Ident scan, so a dedicated collector
// votes argc over `o.m(...)` calls and the method joins the
// static-argv face (extras injected → the checker's arity face
// follows automatically). Mirrors test262
// meth-args-trailing-comma-*. Covers: over-arity values + length,
// unmapped write isolation, and a store-only member escape.
let callCount = 0;
const obj = {
  method() {
    console.log(arguments.length);
    console.log(arguments[0], arguments[1]);
    callCount = callCount + 1;
  }
};
obj.method(42, "TC39");
console.log(callCount);
const ref = obj.method;
console.log(typeof ref);

let seen = 0;
const o = {
  m(a: number) {
    arguments[0] = 9;
    seen = a;
    console.log(arguments.length, arguments[0], arguments[1]);
  }
};
o.m(1, "x");
console.log(seen);
