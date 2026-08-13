// §14.11 — a class written inside a `with` body.
//
// A class is strict code throughout (§11.2.2), which forbids WRITING a
// `with` inside one, but does not take the object environment record
// out of the chain AROUND one. So the question for each class here is
// only whether it reads a name the object could supply: one that does
// is refused (the guard would name the `with` binding, and a nested
// class that captures a local cannot be lifted); a CLOSED one needs no
// guard at all and runs. These are the closed shapes.
//
// `.cts` because `with` only exists under the sloppy goal.

var o: any = { x: 1, hi: "shadowed" };

with (o) {
  // Closed declaration — nothing in the body is free, so the object is
  // never consulted and the class lifts like any other.
  class Plain {
    m(): any {
      return 42;
    }
  }
  console.log(new Plain().m());

  // Closed class EXPRESSION. Its declaration is spliced at TOP LEVEL by
  // the parser, leaving only a name hop at the use site — so the body
  // is already outside the block when the desugar walks it. Closed, so
  // there was nothing to guard.
  const Anon: any = class {
    m(): any {
      return 43;
    }
  };
  console.log(new Anon().m());

  // `extends` a class declared in the body itself. The heritage is
  // evaluated in this scope, so a free parent name could come from the
  // object — but this one is bound right here, which shadows it.
  class Base {
    hi(): any {
      return "base";
    }
  }
  class Sub extends Base {}
  console.log(new Sub().hi());

  // The object still governs ordinary names around the classes, and
  // `hi` is one of them: the method above is a member, not a binding.
  console.log(hi);
  console.log(x);
}
