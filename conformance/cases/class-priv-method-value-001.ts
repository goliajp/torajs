// S2.34 — PRIVATE method value reads (`this.#m`): plain and
// generator, exposed through a public method, invoked via `.call`.
class C {
  #secret() { return 9; }
  *#pg() { yield 8; }
  reveal() { return this.#secret; }
  revealGen() { return this.#pg; }
}
var f = new C().reveal();
console.log(typeof f, f.call(new C()));
var g = new C().revealGen();
var it = g.call(new C());
console.log(typeof g, it.next().value, it.next().done);
