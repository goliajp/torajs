// §28.1.6 / §28.1.13 — Reflect.get / Reflect.set with a receiver run
// the target's accessors with THAT receiver as `this`, whether the
// target is a bound object literal or an inline one; the same
// foreign-receiver promise a prototype breaks.
class B { _s = 0 }
const d = new B();
const lit = { set s(v: any) { (this as any)._s = v + 100 }, get g() { return (this as any)._s + 1 } };
console.log(Reflect.set(lit, "s", 4, d), d._s, Reflect.get(lit, "g", d));
console.log(Reflect.get({ get g() { return (this as any)._s * 2 } }, "g", d));
const o: any = { _s: 7 };
console.log(Reflect.set({ set s(v: any) { (this as any)._s = v } }, "s", 9, o), o._s);
// Without a receiver the target is its own `this`.
const own = { _s: 1, get g() { return this._s + 10 } };
console.log(Reflect.get(own, "g"));
