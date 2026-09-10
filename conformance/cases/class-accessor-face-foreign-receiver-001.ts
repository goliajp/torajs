// §28.1.6 / §28.1.13 — Reflect.get / Reflect.set with a receiver that
// is not an instance of the class run a class accessor with THAT
// receiver as `this`: a plain object, a subclass instance, an object
// under a generic class's accessor. The reified accessor face on the
// prototype carries the same receiver guard the method faces do —
// its own instances take the typed body, everything else the
// receiver-polymorphic twin.
class B { _s = 0; set s(v: number) { this._s = v * 10 } get s() { return this._s + 1 } }
const o: any = { _s: 0 };
console.log(Reflect.set(B.prototype, "s", 4, o), o._s, Reflect.get(B.prototype, "s", o));
const b = new B();
console.log(Reflect.set(B.prototype, "s", 5, b), b._s, Reflect.get(B.prototype, "s", b));
class D extends B {}
const d = new D();
console.log(Reflect.set(B.prototype, "s", 6, d), d._s, Reflect.get(B.prototype, "s", d));
class G<T> { _s: T | undefined; set s(v: T) { this._s = v } get s(): T | undefined { return this._s } }
const g: any = { _s: 0 };
console.log(Reflect.set(G.prototype, "s", 9, g), g._s, Reflect.get(G.prototype, "s", g));
class C { get k() { return 42 } get t() { return (this as any)._t } }
console.log(Reflect.get(C.prototype, "k", o), Reflect.get(C.prototype, "t", { _t: "x" } as any));
