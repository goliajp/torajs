// A plain struct-lane object — `function g(v) { return { a: v } }` —
// answered null to Object.getPrototypeOf through its typed spelling,
// where the same value read through `any` answered %Object.prototype%:
// the typed arm read the class tag and looked up a registered class
// prototype, and a plain object has none. Every struct-layout receiver
// now goes through the runtime classifier, class instance or not.
class C { x = 1; m() { return this.x; } }
class D extends C { y = 2; }
function mk(v: number) { return { a: v }; }
function ret(): { a: number } { return { a: 9 }; }
const c = new C(); const d = new D(); const ca: any = new C(); const da: any = new D();
const s = mk(1); const sa: any = mk(2); const e = {}; const ea: any = {};
console.log("class typed", Object.getPrototypeOf(c) === C.prototype, Object.getPrototypeOf(d) === D.prototype, Object.getPrototypeOf(D.prototype) === C.prototype);
console.log("class any", Object.getPrototypeOf(ca) === C.prototype, Object.getPrototypeOf(da) === D.prototype);
console.log("struct typed", Object.getPrototypeOf(s) === Object.prototype, Object.getPrototypeOf(s) === null, typeof Object.getPrototypeOf(s));
console.log("struct any", Object.getPrototypeOf(sa) === Object.prototype);
console.log("empty typed/any", Object.getPrototypeOf(e) === Object.prototype, Object.getPrototypeOf(ea) === Object.prototype);
console.log("reflect", Reflect.getPrototypeOf(s) === Object.prototype, Reflect.getPrototypeOf(c) === C.prototype);
console.log("nested", Object.getPrototypeOf({ p: mk(3) }.p) === Object.prototype);
console.log("annotated ret", Object.getPrototypeOf(ret()) === Object.prototype);
console.log("chain", Object.getPrototypeOf(Object.getPrototypeOf(s)) === null, Object.prototype.isPrototypeOf(s), s instanceof Object);
