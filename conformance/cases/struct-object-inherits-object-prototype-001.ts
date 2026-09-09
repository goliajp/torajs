// A struct-lane object's member read walked its class prototype chain
// and, when that chain ended (a plain object has none; a base class's
// prototype has no parent), fell to the per-family builtin-method
// reify — a different function cell from the one %Object.prototype%
// owns, so `o.toString === Object.prototype.toString` was false for
// every struct object and class instance while true for a dynobj, and
// the reified cell's call answered "" for a plain object. The chain
// now ends at %Object.prototype% itself, as §10.1.1 says.
function mk(v: number) { return { a: v }; }
class C { x = 1; m() { return this.x; } }
class D extends C { y = 2; }
const o: any = mk(5); const lit: any = { a: 1 }; const OP: any = Object.prototype;
const c: any = new C(); const d: any = new D();
console.log("toString identity", o.toString === OP.toString, lit.toString === OP.toString, c.toString === OP.toString, d.toString === OP.toString);
console.log("names", o.toString.name, typeof o.toString, o.toString.length);
console.log("calls", o.toString.call(o), c.toString.call(c), Object.prototype.toString.call(o), String(o), `${c}`, o + "");
console.log("valueOf", o.valueOf === OP.valueOf, c.valueOf === OP.valueOf, o.valueOf() === o, c.valueOf() === c);
console.log("hasOwn", o.hasOwnProperty === OP.hasOwnProperty, o.hasOwnProperty("a"), c.hasOwnProperty("x"), c.hasOwnProperty("m"), d.hasOwnProperty("x"));
console.log("isPrototypeOf", OP.isPrototypeOf(o), OP.isPrototypeOf(c), C.prototype.isPrototypeOf(d), o.isPrototypeOf === OP.isPrototypeOf);
console.log("propertyIsEnumerable", o.propertyIsEnumerable("a"), c.propertyIsEnumerable("m"), o.propertyIsEnumerable === OP.propertyIsEnumerable);
console.log("toLocaleString", o.toLocaleString === OP.toLocaleString, o.toLocaleString());
console.log("constructor", o.constructor === Object, c.constructor === C, d.constructor === D);
console.log("own method still wins", c.m === C.prototype.m, c.m(), d.m());
console.log("missing", o.nope, c.nope, "nope" in o, "toString" in o, "m" in d);
const typed = mk(7); const ct = new C();
console.log("typed spelling", typed.toString === Object.prototype.toString, ct.toString === Object.prototype.toString, typed.hasOwnProperty("a"));
