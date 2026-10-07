// a subclass overriding both accessor faces is reached through a
// base-typed receiver: parameter, array element, field, return value
class B {
  _v = 10;
  get v(): number { return this._v; }
  set v(x: number) { this._v = x; }
}
class D extends B {
  get v(): number { return this._v * 101; }
  set v(x: number) { this._v = x + 1000; }
}
function show(b: B): number { return b.v; }
function put(b: B, x: number): number { b.v = x; return b._v; }
console.log("poly", show(new B()), show(new D()));
console.log("put", put(new B(), 1), put(new D(), 1));

const arr: B[] = [new B(), new D(), new B()];
let sum = 0;
for (let i = 0; i < arr.length; i++) sum += arr[i].v;
console.log("arr", sum);
for (const e of arr) e.v = 2;
console.log("arr-set", arr.map((e) => e._v).join(","));

class Holder { b: B; constructor(b: B) { this.b = b; } }
const h = new Holder(new D());
console.log("field", h.b.v);
h.b.v = 5;
console.log("field-set", h.b._v);

function make(flag: boolean): B { return flag ? new D() : new B(); }
console.log("ret", make(true).v, make(false).v);

const d: D = new D();
console.log("static", d.v);
d.v = 3;
console.log("static-set", d._v);
