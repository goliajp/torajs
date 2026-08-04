// ES §15.7 private-name lexical scoping across nested classes: an
// inner class body sees the outer class's `#m` (resolve walks OUT
// through the enclosing scopes), and an inner redeclaration shadows
// the outer one. Foreign-brand access throws TypeError.
var C = class {
  #m() {
    return "outer";
  }
  B = class {
    method(o: any) {
      return o.#m();
    }
  };
};
let c: any = new C();
let innerB = new c.B();
console.log(innerB.method(c));

var D = class {
  #m() {
    return "outer";
  }
  E = class {
    #m() {
      return "inner";
    }
    probe(o: any) {
      return o.#m();
    }
  };
};
let d: any = new D();
let e = new d.E();
console.log(e.probe(e));
try {
  console.log(e.probe(d));
} catch (err: any) {
  console.log("caught:", err instanceof TypeError);
}
