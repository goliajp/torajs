// §10.2.1.1 fills a constructor's arguments object from the whole
// argumentsList, including what runs past the declared parameters.
// tr's factory takes the ctor's DECLARED params, so the surplus a
// `new` site wrote died there: `new P(1, 2, 3)` on `constructor(a)`
// answered `arguments.length === 1`. The static-argv face was voting
// on the factory's own inner call — the one site that carries the
// declared arity rather than the program's arguments — and the slots
// it injected had nothing feeding them.
//
// The vote now reads the two sites that DO write the arguments:
// `new C(…)` through `__new_C`, and `super(…)` through the direct
// ctor call; the factory grows the same slots and forwards them.
class P {
  constructor(a: any) {
    console.log("P", arguments.length, arguments[0], arguments[1], arguments[2], a);
  }
}
new P(1, 2, 3);

class Z {
  constructor() {
    console.log("Z", arguments.length, arguments[0], arguments[3]);
  }
}
new Z("a", "b", "c", "d");

class Parent {
  constructor() {
    console.log("Parent", arguments.length, arguments[0], arguments[2]);
  }
}
class Child extends Parent {
  constructor() {
    super(7, 8, 9);
  }
}
new Child();

// A second parent, because the face wants ONE argc per constructor:
// two subclasses calling `super` with different counts is not a
// static argv, and that constructor keeps the declared-params
// approximation (registered, unchanged by this knife).
class Parent2 {
  constructor() {
    console.log("Parent2", arguments.length, arguments[0], arguments[4]);
  }
}
class Spread extends Parent2 {
  constructor() {
    super(1, 2, ...[3, 4, 5]);
  }
}
new Spread();

// a plain method's face is untouched
class M {
  m(a: any) {
    console.log("M", arguments.length, arguments[0], arguments[2], a);
  }
}
new M().m(4, 5, 6);
