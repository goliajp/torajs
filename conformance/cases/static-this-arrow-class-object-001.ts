// An arrow inside a static member body reads the class object. An
// arrow has no `this` of its own, so §10.2.1.2 says it sees the static
// context's -- and the parser's "this means the class here" recording
// therefore reaches inside it, unlike a function expression.
//
// The name that recording mints (`__class_C`) is not a binding: it is a
// sentinel `ssa_lower_ident` answers by calling `class_get(tag)`. But
// lifting the arrow turned it into a free name, and the capture
// resolver only forgave promoted data globals, so it died on `closure
// capture __class_C not in scope`. Sentinels are forgiven for the same
// reason globals are -- the body reads one wherever it stands.
class Counter {
  static total = 0;
  static tag() {
    return "Counter";
  }
  static identity() {
    const f = () => this;
    return f() === Counter;
  }
  static viaMethod() {
    const f = () => this.tag();
    return f();
  }
  // Nested one deep, and reading the class through a closed-over local
  // at the same time.
  static nested(n: number) {
    const twice = () => {
      const inner = () => this.tag() + ":" + n * 2;
      return inner();
    };
    return twice();
  }
  static kind() {
    const f = () => typeof this;
    return f();
  }
}

console.log(Counter.identity(), Counter.viaMethod());
console.log(Counter.nested(3), Counter.kind());

// A function EXPRESSION in the same position binds its OWN `this` --
// the other half of this shape, and the reason the mint must not reach
// inside one.
class Plain {
  static loose() {
    return (function () {
      return typeof this;
    })();
  }
}
console.log(Plain.loose());

// The capturing-nested-class lane reaches static bodies by a different
// route (it drops the recording, so `this` there is ordinary function
// `this`), which has to keep answering.
function make(base: number): any {
  class Boxed {
    n: number;
    constructor(s: number) {
      this.n = s + base;
    }
    static seed() {
      return base;
    }
    static viaArrow() {
      const f = () => this.seed() * 2;
      return f();
    }
  }
  return Boxed;
}
const B: any = make(5);
console.log(new B(1).n, B.viaArrow());
