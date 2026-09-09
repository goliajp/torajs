// §B.3.1 — `{__proto__: v}` is a [[Prototype]] SET, so the object has
// no key by that name. The implicit return-annotation sniff read the
// name as an ordinary field, so an unannotated function returning one
// handed its caller a struct that claimed a `__proto__` slot:
//
//     function f(n) { return {__proto__: n, z: 1}; }
//     f({a:2}).a    →  type error: no member `.a` on type
//                      Struct([("__proto__", ...), ("z", Number)])
//
// and `f({a:1})` crashed outright — the caller reading a layout the
// callee never built. Third name in the same sentinel family as
// `__computed_` and `__spread__`, both of which already take the
// `any` exit.

function withProto(n: any) {
  const o = { __proto__: n };
  return o;
}
const a = withProto({ a: 1 });
console.log(Object.getPrototypeOf(a).a, JSON.stringify(a));

function protoPlusField(n: any) {
  return { __proto__: n, z: 1 };
}
const b = protoPlusField({ a: 2 });
console.log(b.a, b.z, JSON.stringify(b));

// The SHORTHAND `{__proto__}` is a different production and IS an
// ordinary own field (node keeps it and lists it in Object.keys; bun
// drops it), so it stays out of this guard — and out of this fixture,
// because the unannotated spelling of it is broken in its own way and
// is registered separately.

// outside a function the literal was always right — it is the sniff
// that named the sentinel, not the checker
const outer: any = { a: 3 };
const d = { __proto__: outer };
console.log(Object.getPrototypeOf(d).a, JSON.stringify(d));
