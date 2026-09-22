// §10.1.8.1 — reading a method off an exotic subclass instance finds
// the class's own function object before the builtin prototype. The
// call path already did; a property READ answered the builtin method,
// so a borrowed method ran the builtin body and identity failed.
class A extends Array { toString() { return "S"; } count() { return this.length; } }
const a: any = new A();
a.push(1, 2);
const f = a.toString;
console.log(f.call(a), typeof a.toString, a.toString === A.prototype.toString);
console.log(a.push === Array.prototype.push, a.count === A.prototype.count, String(a));
class D extends Date { valueOf() { return 7; } }
const d: any = new D(0);
console.log(d.valueOf === D.prototype.valueOf, +d, d.getTime === Date.prototype.getTime);
class M extends Map { toString() { return "MM"; } }
const m: any = new M();
const g = m.toString;
console.log(m.toString === M.prototype.toString, String(m), g.call(m));
class N extends Number { toFixed() { return "fixed"; } }
const n: any = new N(3);
console.log(n.toFixed === N.prototype.toFixed, n.toFixed());
