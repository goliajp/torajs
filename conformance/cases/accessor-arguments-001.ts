// A class accessor body that reads `arguments` sees the arguments of the
// call that ran it: none for [[Get]], one for [[Set]], and whatever was
// passed when the function is taken out of its descriptor and called —
// on an own instance and on a subclass instance alike.

class C {
  x = 1;
  set s(v: number) { console.log("cset", arguments.length, arguments[0]); }
  get g() { return arguments.length; }
  get v() { return this.x * 10 + arguments.length; }
  set v(n: number) { this.x = n + arguments.length; }
}
const c = new C();
c.s = 7;
console.log("cget", c.g);
c.v = 3;
console.log("c.v", c.v);
c.v += 1.5;
console.log("compound", c.x);
const cd = Object.getOwnPropertyDescriptor(C.prototype, "v");
console.log("proto get", cd.get.call(c, 1, 2));
cd.set.call(c, 5, 6, 7);
console.log("proto set", c.x);
const sd = Object.getOwnPropertyDescriptor(C.prototype, "s");
sd.set.call(c, 1, 2, 3);

class D extends C {}
const d = new D();
d.s = 4;
console.log("heir get", cd.get.call(d, 1));
cd.set.call(d, 2, 3);
console.log("heir set", d.x);

class G<T> {
  item: T;
  constructor(t: T) { this.item = t; }
  get first() { return arguments.length; }
  set first(t: T) { this.item = t; console.log("G set", arguments.length, arguments[0]); }
}
const g = new G<number>(1);
g.first = 2;
console.log("G get", g.first, g.item);
