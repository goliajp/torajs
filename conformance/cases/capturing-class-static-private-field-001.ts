// A static private field of a class written inside a function is added to
// the class object when the class is defined, so its static members can read
// and write it.

(function () {
  class Counter {
    static #value = 42;
    static get v() {
      return Counter.#value;
    }
    static set v(value) {
      Counter.#value = value;
    }
  }
  console.log(Counter.v);
  Counter.v = 43;
  console.log(Counter.v);
})();

function g() {
  class Box {
    static #value = 1;
    static read() {
      return this.#value;
    }
    static write() {
      Box.#value = 5;
    }
  }
  console.log(Box.read());
  Box.write();
  console.log(Box.read());
}
g();
