// §15.7 — every PrivateName a class body declares is in scope for the
// whole body, so `this.#f` where `#f` is STATIC is legal code. What it
// is not is a match: §7.3.32 PrivateElementFind answers empty and the
// reference is a RUNTIME TypeError. tr rejected it at check time,
// because the question it asked was "is this name on the INSTANCE
// member list" — the same miss an undeclared name produces, which is
// why a static one looked like a violation.
//
// The early error still belongs to a name no enclosing class declares;
// the parser answers that one.
var C = class {
  static #f() {
    return 42;
  }
  static #v = 7;
  g() {
    return this.#f();
  }
  h() {
    return this.#v;
  }
};

// Called with the CLASS as the receiver, the brand matches.
console.log("bound-method", new C().g.call(C));
console.log("bound-field", new C().h.call(C));

function reports(label, f) {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, e.constructor.name);
  }
}
// Called on an instance, the brand check fails.
reports("instance-method", function () {
  return new C().g();
});
reports("instance-field ", function () {
  return new C().h();
});

// A class DECLARATION behaves the same way.
class D {
  static #s = 5;
  read() {
    return this.#s;
  }
}
console.log("decl-bound", new D().read.call(D));
reports("decl-instance  ", function () {
  return new D().read();
});

// An instance private is untouched by any of this.
class E {
  #i = 3;
  get() {
    return this.#i;
  }
}
console.log("instance-private", new E().get());
