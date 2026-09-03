// §15.7.1 ClassElementName (rotation 575). The refusals — a static
// element named `prototype`, a field named `constructor`, a
// getter/setter/generator/async named `constructor` — are negative
// cases and live in test262. This fixture is the other side: the
// shapes near those gates that are LEGAL and must keep working.
//
// A STATIC element spelled `constructor` is an ordinary static
// member: PrototypePropertyNameList collects only the non-static
// ones, so none of these is the class's constructor. tr refused the
// plain and getter spellings before this rotation.
class S {
  static constructor(): number { return 7; }
}
console.log(S.constructor());
class S2 {
  static get constructor(): number { return 8; }
  static set constructor(v: number) { /* accepted, never the ctor */ }
}
console.log(S2.constructor);
class S3 {
  static *constructor(): any { yield 1; }
  static async asyncCtorLike(): Promise<number> { return 2; }
}
console.log([...S3.constructor()]);
// A class may carry both — they are different members.
class S4 {
  v: number;
  constructor() { this.v = 3; }
  static constructor(): number { return 9; }
}
console.log(new S4().v, S4.constructor());
// `prototype` is forbidden only on the STATIC side.
class P {
  prototype(): number { return 11; }
  get p2(): number { return 12; }
  *pg(): any { yield 13; }
}
const p = new P();
console.log(p.prototype(), p.p2, [...p.pg()]);
// A private name is mangled before the rule looks at it, so
// `static #prototype` is untouched by the static-`prototype` gate.
class Q {
  static #prototype = 14;
  static read(): number { return Q.#prototype; }
}
console.log(Q.read());
// And a plain constructor plus an ordinary static method still work.
class R {
  w: number;
  constructor(w: number) { this.w = w; }
  static make(w: number): R { return new R(w); }
}
console.log(R.make(15).w);
