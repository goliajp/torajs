// generic-parent default ctor — ES §15.7.10 implicit ctor across a
// generic heritage: the synthesized `constructor(...){ super(...) }`
// substitutes the ancestor ctor param anns via the written heritage
// type arguments (rotation 413 blade 1).
class Box<T> {
  v: T;
  constructor(x: T) { this.v = x; }
  tag(): string { return "v=" + this.v; }
}
// direct: implicit default ctor over a generic parent
class NumKid extends Box<number> {}
class StrKid extends Box<string> {}
const n = new NumKid(21);
console.log(n.v);
console.log(n.tag());
const s = new StrKid("hi");
console.log(s.v);
console.log(s.tag());
// chained: substitution composes across hops (Mid re-spells T as U)
class Mid<U> extends Box<U> {
  constructor(x: U) { super(x); }
}
class ChainKid extends Mid<number> {}
const c = new ChainKid(7);
console.log(c.v);
console.log(c.tag());
// a generic parent with a field-init-only body still runs super
class Base<T> {
  w: number = 5;
  constructor() {}
}
class InitKid extends Base<string> {}
console.log(new InitKid().w);
