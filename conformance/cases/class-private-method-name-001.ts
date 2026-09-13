// A private method's function name is its Private Name's description
// (§15.7.14 → §10.2.9 SetFunctionName): `#m`, not the compiler's
// `__priv_<C>__m` spelling — for a plain, generator, async and static
// private method alike (test262 private-methods/prod-private-*).
class C {
  #m() { return 42; }
  *#g() { yield 1; }
  async #a() { return 1; }
  static #s() { return 2; }
  get ref(): any { return this.#m; }
  get gref(): any { return this.#g; }
  get aref(): any { return this.#a; }
  static sref(): any { return C.#s; }
  m2() { return 1; }
}
const c = new C();
console.log("private-names", c.ref.name, c.gref.name, c.aref.name, C.sref().name, c.m2.name, c.ref() === 42);
