// §13.4.4–13.4.7 UpdateExpression on a Super Reference: the read is off the base (the parent
// prototype) with this as receiver, the step is ToNumeric in the operand's own domain, the
// write lands on this (§9.1.9), and the expression answers the old value (postfix) or the new
// one (prefix). A later super read still sees the base, not the freshly written own property.
class B { get g() { return (this as any)._g; } set g(v: number) { (this as any)._g = v * 100; } }
(B.prototype as any).p = 1;
(B.prototype as any).q = "7";
(B.prototype as any).big = 10n;
class D extends B {
  m() { return [super.p++, (this as any).p, ++super.p, (this as any).p, super.p--, --super.p, (this as any).p]; }
  s() { return [super.q++, (this as any).q, typeof (this as any).q]; }
  bigStep() { return [super.big++, (this as any).big, --super.big, (this as any).big]; }
  acc() { (this as any)._g = 3; return [super.g++, (this as any)._g, ++super.g, (this as any)._g]; }
  k(name: string) { return [super[name]++, (this as any)[name]]; }
}
const d = new D();
console.log(d.m().join(","), Object.getOwnPropertyDescriptor(d, "p") !== undefined, (B.prototype as any).p);
console.log(d.s().join(","));
console.log(d.bigStep().join(","));
console.log(d.acc().join(","));
console.log(new D().k("p").join(","));
var order: string[] = [];
var proto: any = { p: 1 };
var proto2: any = { p: -1 };
class E { h() { return ++super[key]; } }
var key: any = { toString() { order.push("key"); Object.setPrototypeOf(E.prototype, proto2); return "p"; } };
Object.setPrototypeOf(E.prototype, proto);
console.log(new E().h(), order.join(","));
class F { static c = 5; }
class G extends F { static bump() { return [super.c++, (G as any).c, F.c]; } }
console.log(G.bump().join(","));
