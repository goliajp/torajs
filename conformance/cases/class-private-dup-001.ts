// §15.7.1 — PrivateBoundIdentifiers of a ClassElementList may not
// repeat, with exactly one exception: a name used once for a getter
// and once for a setter, both static or both non-static. The
// refusals are test262 negatives; this fixture is the exception and
// the shapes that only LOOK like repeats.
//
// The legal pair, non-static.
class A {
  #v = 1;
  get #x(): number { return this.#v; }
  set #x(n: number) { this.#v = n; }
  bump(): number { this.#x = this.#x + 10; return this.#x; }
}
console.log(new A().bump());
// The legal pair, static.
class B {
  static #v = 2;
  static get #x(): number { return B.#v; }
  static set #x(n: number) { B.#v = n; }
  static bump(): number { B.#x = B.#x + 20; return B.#x; }
}
console.log(B.bump());
// Two classes may each declare `#x` — the names are per-class.
class C1 { #x = 3; read(): number { return this.#x; } }
class C2 { #x = 4; read(): number { return this.#x; } }
console.log(new C1().read(), new C2().read());
// A subclass may declare `#x` even though its parent has one — two
// ClassElementLists, two sets of PrivateBoundIdentifiers.
class D1 { #x = 5; readD1(): number { return this.#x; } }
class D2 extends D1 { #x = 6; readD2(): number { return this.#x; } }
const d = new D2();
console.log(d.readD1(), d.readD2());
// A getter alone, a setter alone, and a plain private method — one
// declaration each, nothing to pair with.
class E {
  #n = 7;
  get #g(): number { return this.#n; }
  #m(): number { return this.#n + 1; }
  read(): number { return this.#g + this.#m(); }
}
console.log(new E().read());
// A private name and a PUBLIC one spelled the same are different
// members entirely.
class F {
  #x = 8;
  x = 9;
  read(): number { return this.#x + this.x; }
}
console.log(new F().read());
