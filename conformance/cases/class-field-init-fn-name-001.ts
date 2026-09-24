// A class field's initializer is named by the field (§15.7.10): an anonymous
// function in a static, instance or private field takes the field's name,
// and a private one takes its description "#x".
class C {
  static #field = () => "T";
  static field = function () { return 42; };
  inst = function () {};
  #priv = () => 1;
  static get pf() { return C.#field; }
  get pp() { return this.#priv; }
}
const c = new C();
console.log(C.pf.name, C.field.name, c.inst.name, c.pp.name);
var D = class {
  static f = this;
  static g = () => this;
  static h = function () {};
  static k = class {};
};
console.log(D.f === D, D.g() === D, D.h.name, D.k.name);
