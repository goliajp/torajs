// [[HasProperty]] on a class instance walks its class prototype, also
// behind a Proxy with no has trap and through Reflect.has.
const sym = Symbol("s");
class C { m() { return 1; } get g() { return 2; } [sym]() { return 3; } x = 1; }
const c = new C();
console.log(Reflect.has(c, "m"), Reflect.has(c, "g"), Reflect.has(c, "x"), Reflect.has(c, "zz"), Reflect.has(c, "toString"));
console.log(sym in c, Reflect.has(c, sym));
const o = { a: 1 };
const p = { b: 2, [sym]: 3 };
Object.setPrototypeOf(o, p);
console.log(Reflect.has(o, "b"), Reflect.has(o, sym), sym in o, Reflect.has(o, "toString"));
const px = new Proxy(c, {});
console.log("m" in px, Reflect.has(px, "g"));
