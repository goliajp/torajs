// K6-1 — class expressions in method bodies of a class with private
// elements: a capture-free one carries its own brand per evaluation; one
// reading an outer local takes the runtime-value lane and reaches the
// outer instance's private field through the captured `self`.
class Outer {
  #x = 42;
  make(): any { return class { #v = 7; read(o: any): number { return o.#v; } }; }
  peek(): any { const self = this; return class { g(): number { return self.#x; } }; }
}
const o = new Outer();
const A: any = o.make(), B: any = o.make();
const a = new A(), b = new B();
console.log(a.read(a));
try { a.read(b); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
const G: any = o.peek();
console.log(new G().g());
