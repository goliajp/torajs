// an overridden accessor reading `arguments`, called with extra
// arguments through its property descriptor
class B {
  get v(): number { return arguments.length; }
  set v(x: number) { console.log("B.set", arguments.length, arguments[0], arguments[1]); }
}
class D extends B {
  get v(): number { return arguments.length + 100; }
  set v(x: number) { console.log("D.set", arguments.length, arguments[0], arguments[1]); }
}
const o = new D();
const dd = Object.getOwnPropertyDescriptor(D.prototype, "v")!;
console.log(dd.get!.call(o, 1, 2));
dd.set!.call(o, 5, 6);
const bd = Object.getOwnPropertyDescriptor(B.prototype, "v")!;
console.log(bd.get!.call(o, 1, 2, 3));
bd.set!.call(o, 7);
function show(b: B): number { return b.v; }
function put(b: B, x: number): void { b.v = x; }
console.log(show(new B()), show(new D()));
put(new B(), 1);
put(new D(), 2);
const d = new D();
console.log(d.v);
d.v = 3;
