// An accessor defined on a function object runs with that function as
// `this`, for the setter as much as for the getter.

const f: any = function () {};
Object.defineProperty(f, "v", {
  get() {
    return this === f;
  },
  set(x) {
    console.log("set", this === f, typeof this, x);
  },
});
console.log("get", f.v);
f.v = 1;

function g() {}
Object.defineProperty(g, "w", {
  set(x) {
    console.log("set", this === g, typeof this, x);
  },
});
(g as any).w = 2;

(function () {
  class K {
    static #value = 0;
    static set v(x) {
      console.log("static set", this === K);
      this.#value = x;
    }
    static get v() {
      return K.#value;
    }
  }
  K.v = 7;
  console.log(K.v);
})();
