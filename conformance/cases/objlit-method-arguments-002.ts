// `typeof arguments` inside a method of an object literal written in a
// function is "object": the method has an arguments object of its own,
// whatever happens to the literal afterwards.
function t(z: number) {
  const o = {
    m() {
      return typeof arguments + ":" + arguments.length;
    },
  };
  (o as any).q = 3;
  return o.m() + "/" + arguments.length;
}
console.log(t(1));

// length-only methods answer each call's own count, also through any
function u(z: number) {
  const o = {
    m(a?: number, b?: number) {
      return arguments.length;
    },
    k() {
      return typeof arguments;
    },
  };
  const r: any = o;
  return [o.m(), o.m(1), o.m(1, 2), r.m(1, 2, 3), o.k(), r.k(9), arguments.length].join(",");
}
console.log(u(5));

// the method read off as a value and called, and spread calls
function v() {
  const o = {
    m(a?: number) {
      return arguments.length;
    },
  };
  const f = o.m;
  const xs = [1, 2, 3];
  return [f(), f(1), o.m(...xs), arguments.length].join(",");
}
console.log(v.call(null, 8, 9));

// a function-level `typeof arguments` next to a length read
function w(a: number) {
  return typeof arguments + ":" + arguments.length;
}
console.log(w(1), (w as any)(), (w as any)(1, 2));
