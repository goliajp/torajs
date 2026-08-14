// Two nested classes share a name, so the hoist α-renames one of them.
// A static body's `this` is recorded by the name the source used, and
// that recording has to move with the rename — otherwise it mints a
// binding nobody declares, and `typeof` of an unresolvable name answers
// "undefined" instead of saying so.
function first(a: number) {
  class K {
    static tag() {
      return "k" + a;
    }
  }
  return K.tag();
}

const second = (function () {
  class K {
    static who() {
      return typeof this;
    }
    static name_() {
      return this.who() + "!";
    }
  }
  return K.name_();
})();

console.log(first(1));
console.log(second);
