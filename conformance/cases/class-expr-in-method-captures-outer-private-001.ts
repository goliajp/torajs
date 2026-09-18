// K6-1 — the test262 shapes (privatename-valid-no-earlyerr,
// privatefieldget-success-1, privatefieldget/-set-typeerror-5): a class
// expression returned from an instance method reads the outer instance's
// private field through a captured local; an inner `#x` shadows the outer.
class outer { #x = 42; f(): any { var self = this; return class inner { g(): number { return self.#x; } }; } }
console.log(new (new outer().f())().g());

class Outer { #x = 42; f(): any { var self = this; return class extends Outer { g(): number { return self.#x; } }; } }
const Inner: any = new Outer().f();
console.log(new Inner().g(), new Inner() instanceof Outer);

class Shadow {
  #x = 42;
  f(): any { var self = this; return class extends Shadow { #x = 'not42'; g(): any { return self.#x; } s(): void { self.#x = 1; } }; }
}
const SI: any = new Shadow().f();
try { new SI().g(); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
try { new SI().s(); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }

// a class declaration inside a method of a hoisted nested class
function mk(): any { class A { m(): any { class B { t(): string { return "b"; } } return B; } } return new A().m(); }
const B: any = mk();
console.log(new B().t());
