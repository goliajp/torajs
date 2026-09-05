// §14.2.3 / §16.1.7 — a class declaration is not a constant
// declaration, so the scope around it holds a MUTABLE binding under
// the source's spelling. tr had only the other one: the immutable
// binding §15.7.14 step 3 puts inside the class scope, which every
// reference was resolved to, so a write outside the class was a
// compile-time reject of a legal program.
class D {}
D = 1 as any;
console.log(D, typeof D);

// The two bindings are separate, and this is the difference that
// makes them two: a reference captured inside the class still answers
// the class after the outer name has been written.
class C {
  field: any = () => C;
}
const kept: any = C;
C = null as any;
console.log(new kept().field() === kept, C);

// Writing the name from inside the class is still the immutable one.
class E {
  static probe() {
    try {
      E = 1 as any;
    } catch (e: any) {
      return e.constructor.name;
    }
    return "no throw";
  }
}
console.log(E.probe());

// The outer binding is a normal module binding, so a function body
// reads and writes the same one the top level does.
class F {}
function rebind() {
  F = 7 as any;
  return F;
}
console.log(rebind(), F);

// Nothing else about a class moves: static members, inheritance,
// `instanceof` and `.name` all read as before.
class Base {
  greet() {
    return "base";
  }
}
class Derived extends Base {
  static tag = "d";
  greet() {
    return super.greet() + "+derived";
  }
}
const inst = new Derived();
console.log(inst.greet(), Derived.tag, Derived.name, inst instanceof Base);
