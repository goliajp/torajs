// M5.N knife 1 — `class C extends Object` (§19.1.1): the Object
// constructor is designed to be subclassable, and under an active
// newTarget its [[Construct]] is OrdinaryCreateFromConstructor —
// exactly the base-class factory. The desugar strips the builtin
// parent to base-class shape: super(...) evaluates its arguments
// for effects (§13.3.7.1) and contributes nothing; an explicit
// ctor with no super() throws ReferenceError at the implicit
// return (§9.2.2 this-TDZ, the append_no_super_throw shape).
class Obj extends Object {}
const o = new Obj();
console.log(o instanceof Obj, o instanceof Object);
console.log(Object.getPrototypeOf(o) === Obj.prototype);
console.log(Object.getPrototypeOf(o) === Object.prototype);

class V extends Object {
  valueOf(): number { return 42 }
}
const v = new V();
console.log(v.valueOf());

function eff(tag: string): number { console.log("eff:" + tag); return 1 }
class B extends Object {
  x: number;
  constructor() {
    super(eff("a"), eff("b"));
    this.x = 5;
  }
}
const b = new B();
console.log(b.x);

class NoSuper extends Object {
  constructor() {
    const y: number = 1;
  }
}
try {
  new NoSuper();
  console.log("BAD: no throw");
} catch (e) {
  console.log("nosuper:", e instanceof ReferenceError);
}

class F extends Object {
  f: number = 3;
}
console.log(new F().f);
