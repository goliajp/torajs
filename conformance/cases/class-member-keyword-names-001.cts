// §15.4 ClassElementName is a PropertyName, and §12.7.6 PropertyName
// is an IdentifierName — every keyword included. Two lookaheads read
// a shorter list than that: the one that decides whether `static` is a
// modifier or the member's own name (a hand-copied subset of
// `keyword_property_name`, missing `let` / `const` / `type` / `await`
// / `var` / `import` / `export` / `null` / `true` / `false` /
// `extends` / `super` / `void`), and the one that recognises a
// `get` / `set` prefix (which admitted only `Ident`).
class A {
  static let = 1;
  static type = 2;
  static await = 3;
  static const = 4;
  static void = 5;
  static null = 6;
  static true = 7;
  static import = 8;
}
console.log(A.let, A.type, A.await, A.const, A.void, A.null, A.true, A.import);

class B {
  get if() { return 9; }
  get let() { return 10; }
  get yield() { return 11; }
  get async() { return 12; }
  get type() { return 13; }
  get await() { return 14; }
  set super(v) { this._s = v; }
  static get const() { return 15; }
}
var b = new B();
console.log(b.if, b.let, b.yield, b.async, b.type, b.await);
b.super = 16;
console.log(b._s, B.const);

// The shapes those lookaheads exist to protect still work: a member
// actually NAMED `static` / `get` / `set`, and a static generator.
class C {
  static() { return 17; }
  get() { return 18; }
  set() { return 19; }
  static *gen() { yield 20; }
}
var c = new C();
console.log(c.static(), c.get(), c.set(), C.gen().next().value);
