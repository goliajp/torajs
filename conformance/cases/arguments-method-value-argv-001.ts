// RFC 20260801-arguments-method-face knife 4a — a class method
// reached only through member-VALUE reads rides the runtime argv
// face: the boxed adapter's true argc/argv feed the injected
// synthetics, so `arguments` answers the real call, not the
// declared arity.

// private method behind a getter, invoked with a receiver
class A {
  #m() {
    console.log(arguments.length, arguments[0], arguments[1]);
  }
  get method() {
    return this.#m;
  }
}
new A().method(42, "TC39");

// prototype-read escape, bare call (this = undefined, body this-free).
// NOTE: method names are unique per class across this file — two
// classes sharing a short name are a sibling collision, which the
// admit correctly refuses (the `__dispatch_`/sibling lanes call by
// the old signature).
class B {
  mB() {
    console.log(arguments.length, arguments[0], arguments[1], arguments[2]);
  }
}
var refB = B.prototype.mB;
refB(7, "x");

// generator method escape — the class-side forwarder carries the
// true argv into the factory through its [...arguments] tail
class G {
  *gm() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  }
}
var refG = G.prototype.gm;
refG(9, "gen").next();

// private generator behind a getter
class D {
  *#pg() {
    console.log(arguments.length, arguments[0]);
    yield 2;
  }
  get method() {
    return this.#pg;
  }
}
new D().method("only").next();

// under-arity escape call — beyond-argc reads answer undefined
class E {
  mE() {
    console.log(arguments.length, arguments[0], arguments[1]);
  }
}
var refE = E.prototype.mE;
refE(1);
