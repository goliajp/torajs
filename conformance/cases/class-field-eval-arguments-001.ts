// §19.2.1.1 step 12 — PerformEval carries `inClassFieldInitializer`
// into the eval and re-states §15.7.1 over the eval'd body, because
// the text arrives at that position too. tr enforced §15.7.1 on the
// initializer a program writes literally (the parser does it) and
// never on the half that comes through a direct eval, so
// `class C { x = eval('arguments') }` just ran.
//
// The verdict belongs at the call, not at parse time: the spec throws
// when the initializer RUNS, and the body must not execute.
//
// The control group is the point of the pass: a constructor, a
// method and a function expression each bind their own `arguments`,
// so an eval in those positions is legal and must stay legal — the
// marking walk pierces arrows and stops at all three.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
let executed: any = false;

class A {
  x: any = eval("executed = true; arguments;");
}
console.log("instance field   ", attempt(() => new A()), "body ran:", executed);

executed = false;
class B {
  #x: any = eval("executed = true; arguments;");
  read(): any {
    return this.#x;
  }
}
console.log("private field    ", attempt(() => new B()), "body ran:", executed);

// An arrow in a field initializer has no `arguments` of its own, so
// the prohibition follows it down — however deeply.
executed = false;
class C {
  x: any = () => {
    const t: any = () => {
      eval("executed = true; arguments;");
    };
    t();
  };
}
console.log("nested arrows    ", attempt(() => (new C().x as any)()), "body ran:", executed);

// …and an arrow written INSIDE the eval source is pierced the same
// way: ContainsArguments recurses into ArrowFunction.
class D {
  x: any = eval("() => arguments;");
}
console.log("arrow in source  ", attempt(() => new D()));

// A static field initializer runs at class definition time and is
// covered — it is a field initializer.
console.log("static field     ", attempt(() => {
  const K: any = class {
    static x: any = eval("arguments");
  };
  return K.x;
}));
// A static block is NOT covered: §19.2.1.1 keys on
// `inClassFieldInitializer`, which only a field initializer sets.
// (tr cannot run `arguments` through an eval there at all yet, so
// the shape stays out of this fixture — registered, not silent.)

// ---- control group: positions that DO bind their own `arguments` ----
class E {
  v: any;
  constructor() {
    this.v = eval("typeof arguments");
  }
}
console.log("ctor is not a field", attempt(() => new E().v));

class F {
  m(): any {
    return eval("typeof arguments");
  }
}
console.log("method            ", attempt(() => new F().m()));

// A function expression inside a field initializer binds its own
// `arguments`, so the walk stops there and the eval is legal.
class G {
  x: any = (function () {
    return eval("typeof arguments");
  })();
}
console.log("fn expr in field  ", attempt(() => new G().x));

// An ordinary function, nothing to do with classes.
function plain(): any {
  return eval("typeof arguments");
}
console.log("plain function    ", attempt(() => plain()));

// A field initializer with no `arguments` in it is untouched.
class H {
  x: any = eval("1 + 1");
}
console.log("innocent field    ", attempt(() => new H().x));

// The word only counts as an IdentifierReference — a property named
// `arguments` is not one.
class I {
  x: any = eval("({ arguments: 5 }).arguments");
}
console.log("property name     ", attempt(() => new I().x));
