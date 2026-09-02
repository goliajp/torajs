// 563-05 — a class EXPRESSION has no name in the source, so the
// parser mints a `__ClassExpr_<id>` binding for it. That synth was
// reaching three faces the user reads: `.name`, the class object's
// printed form, and the printed prefix of an instance. §8.4
// NamedEvaluation gives the class the name of the binding it is
// assigned to, and the empty string when there is none — never the
// implementation's spelling.
console.log(JSON.stringify((class {}).name));
console.log(class {});
console.log(new (class { x = 1 })());

// A binding position names it (this half already worked for `.name`,
// but the instance prefix printed the synth).
const A = class { x = 1 };
console.log(JSON.stringify(A.name), A, new A());

// An inner name wins over the binding (§15.7.4 — the class's own
// BindingIdentifier).
const B = class Named { y = 2 };
console.log(JSON.stringify(B.name), B, new B());

// The binding of a subclass, and its `extends` face.
class Base {}
const C = class extends Base { z = 3 };
console.log(JSON.stringify(C.name), C, new C());

// Anonymous, held without any binding position.
const held: any[] = [class { w = 4 }];
console.log(JSON.stringify(held[0].name), held[0], new held[0]());

// Nested: a class expression inside another class's field.
const D = class { inner = class Inner {} };
console.log(JSON.stringify(new D().inner.name), new D().inner);

// 564-02 — the other §8.4 NamedEvaluation positions: an assignment,
// an object-literal property, and a class field initializer. Each
// names the anonymous class after the binding it lands in.
let E: any;
E = class { v = 5 };
console.log(JSON.stringify(E.name), E, new E());

const obj = { m: class { v = 6 }, n: class Q {} };
console.log(JSON.stringify(obj.m.name), obj.m, new obj.m());
console.log(JSON.stringify(obj.n.name));

class Holder { inner = class {}; static outer = class {} }
console.log(JSON.stringify(new Holder().inner.name), new Holder().inner);
console.log(JSON.stringify(Holder.outer.name));

// A member / index assignment target names nothing (§8.4 applies to
// an IdentifierReference), and neither does a compound assign.
const box: any = {};
box.k = class { v = 7 };
console.log(JSON.stringify(box.k.name), box.k);

// Still open, measured here and not about the synth:
//   564-03 — `console.log(class extends Object {})` prints
//     `[class (anonymous)]` where bun prints
//     `[class (anonymous) extends Object]`: the `extends` face names
//     a superclass only when it is a USER class.
