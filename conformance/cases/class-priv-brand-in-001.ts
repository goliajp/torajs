// ES2022 §13.10 ergonomic brand check — `#x in o` answers whether
// the receiver's class declared the private element (method or
// field), sees outer names from a nested class lexically, and throws
// TypeError for a non-Object rhs (step 5).
class C {
  #m() {
    return 1;
  }
  #f = 2;
  static probe(o: any) {
    return [#m in o, #f in o];
  }
  bare(o: any) {
    return #f in o;
  }
  B = class {
    inner(o: any) {
      return #m in o;
    }
  };
}
let c: any = new C();
console.log(C.probe(c));
console.log(C.probe({}));
let b = new c.B();
console.log(b.inner(c));
console.log(c.bare(c));
try {
  C.probe(42);
} catch (e: any) {
  console.log("caught:", e instanceof TypeError);
}
