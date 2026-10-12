// A nested class that reads its enclosing scope sees the new.target the
// construction handed it, at every link of a derived constructor chain.
function make(tag: string) {
  class Base {
    constructor() {
      console.log(tag, "Base", new.target === Base, new.target === Mid, new.target === Leaf);
    }
  }
  class Mid extends Base {
    constructor() {
      super();
      console.log(tag, "Mid", new.target === Mid, new.target === Leaf);
    }
  }
  class Leaf extends Mid {}
  class Dflt {
    constructor(x: any = 5) {
      console.log(tag, "Dflt", new.target === Dflt, x);
    }
  }
  new Base();
  new Mid();
  new Leaf();
  new Dflt();
  new Dflt(2);
  const r = Reflect.construct(Base, [], Leaf);
  console.log(tag, r instanceof Leaf, Object.getPrototypeOf(r) === Leaf.prototype);
}
make("a");
make("b");

// the same chain over a class that captures nothing
function plain() {
  class P {
    constructor() {
      console.log("P", new.target === Q);
    }
  }
  class Q extends P {}
  new Q();
}
plain();
