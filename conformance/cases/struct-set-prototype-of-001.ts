// Object.setPrototypeOf / Reflect.setPrototypeOf / __proto__ on struct
// instances (object literals, class instances) re-parent them.
const base = { x: 42, greet(this: any) { return "hi " + this.name; } };
const o = { name: "o" };
Object.setPrototypeOf(o, base);
console.log(Object.getPrototypeOf(o) === base, (o as any).x, (o as any).greet());
console.log(base.isPrototypeOf(o), Object.keys(o).join(","));

const n = { a: 1 };
Object.setPrototypeOf(n, null);
console.log(Object.getPrototypeOf(n) === null, (n as any).toString, (n as any).a);

class C { m() { return "C.m"; } get g() { return "C.g"; } }
class D { m() { return "D.m"; } get g() { return "D.g"; } }
const c: any = new C();
Object.setPrototypeOf(c, D.prototype);
console.log(c.m(), c.g, Object.getPrototypeOf(c) === D.prototype);
console.log(Reflect.setPrototypeOf(c, C.prototype), c.m(), c.g);

const a = { k: 1 };
const b = { j: 2 };
Object.setPrototypeOf(a, b);
try {
  Object.setPrototypeOf(b, a);
  console.log("no cycle error");
} catch (e) {
  console.log("cycle", e instanceof TypeError);
}
console.log(Reflect.setPrototypeOf(b, a));

const fr = Object.preventExtensions({ q: 1 });
try {
  Object.setPrototypeOf(fr, base);
} catch (e) {
  console.log("non-extensible", e instanceof TypeError);
}
console.log(Reflect.setPrototypeOf(fr, base), Reflect.setPrototypeOf(fr, Object.prototype));

const u = { z: 1 };
(u as any).__proto__ = base;
console.log((u as any).x, Object.getPrototypeOf(u) === base);

function F() {}
Object.setPrototypeOf(F, Object.prototype);
console.log(Object.getPrototypeOf(F) === Object.prototype);
