// Rotation 273 knife 5 — static methods (`__sm_` forwarders, __this
// first with an undefined receiver) join the method static-argv
// face, and the fn-value alias devirt feeds the receiver slot when
// it bypasses the relay. A static generator method's arguments
// answer the true call-site argv on both the direct and the
// escaped-alias path.

// direct static gen call, over-arity
class A {
  static *gm() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  }
}
A.gm(42, "TC39").next();

// escaped alias + direct call coexisting — each answers its own argv
class B {
  static *gm() {
    console.log(arguments.length, arguments[0], arguments[1]);
    yield 1;
  }
}
var ref = B.gm;
ref(9, "z").next();
B.gm(42, "TC39").next();

// plain static method, uniform over-arity direct calls
class C {
  static sm(a: number) {
    console.log(arguments.length, arguments[0], arguments[1]);
  }
}
C.sm(1, "b");
C.sm(2, "c");

// static async generator, escaped alias
class D {
  static async *am() {
    console.log(arguments.length, arguments[0]);
    yield 1;
  }
}
var refD = D.am;
refD("only")
  .next()
  .then(() => console.log("done"));
