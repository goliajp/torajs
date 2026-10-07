// A class method or getter that reads `arguments`, called with another
// receiver through `call` / `apply` or taken out of its descriptor,
// sees the arguments of that call.

class C {
  v = 1;
  m(a: number) { return this.v + ":" + arguments.length + ":" + arguments[1]; }
  get g() { return this.v + arguments.length; }
}
const c = new C();
console.log(c.m(1, 2));
const other = { v: 9 };
console.log(C.prototype.m.call(other, 3, 4, 5));
console.log((C.prototype.m as any).apply(c, [6, 7]));
const d = Object.getOwnPropertyDescriptor(C.prototype, "g");
console.log(d.get.call(other, 1, 2));
