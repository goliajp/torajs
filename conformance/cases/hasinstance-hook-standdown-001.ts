// §13.10.2 step 2 calls the target's `@@hasInstance` handler when it
// has one, for ANY operand. The class-tag ladder is blind to a handler
// nothing in the class body declares — a `defineProperty` installs one
// just as well, and the ladder kept answering from the hierarchy.

class C {
  x = 1;
}
const c = new C();
console.log("before", c instanceof C);
Object.defineProperty(C, Symbol.hasInstance, {
  value: function (v: any) {
    return typeof v === "number";
  },
});
console.log("after", c instanceof C, (3 as any) instanceof C);

// a subclass inherits the handler through its constructor's
// [[Prototype]]
class D extends C {}
console.log("subclass", new D() instanceof D, (4 as any) instanceof D);

// a class that declares its own handler was already right, and stays
class E {
  static [Symbol.hasInstance](v: any): boolean {
    return v === "yes";
  }
}
console.log("declared", ("yes" as any) instanceof E, ({} as any) instanceof E);
