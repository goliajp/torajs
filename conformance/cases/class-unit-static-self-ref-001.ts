// A class written in a function body whose static members name the
// class itself. Each call evaluates a new class (§15.7.14), and inside
// the class body the name is that evaluation's class object, so a
// static private field read through it sees its own initializer.
function mk() {
  class K {
    static #count = 1;
    static base = 10;
    static twice = K.base * 2;
    static #m() {
      return K.#count;
    }
    static get v() {
      return K.#m() + K.twice;
    }
    static bump() {
      K.#count++;
      return K.#count;
    }
    static self() {
      return K;
    }
    static later() {
      return [1, 2].map((x) => x + K.#count);
    }
  }
  return K;
}
const A = mk();
const B = mk();
console.log(A === B, A.v, B.v);
console.log(A.bump(), A.bump(), B.bump());
console.log(A.self() === A, B.self() === B, A.self() === B);
console.log(A.later().join(","), B.later().join(","));

(function () {
  class C1 {
    static #accessor = 42;
    static test() {
      return C1.#accessor;
    }
  }
  class C3 {
    static #value = 42;
    static get #accessor() {
      return C3.#value;
    }
    static set #accessor(value) {
      C3.#value = value;
    }
    static test() {
      const before = C3.#accessor;
      C3.#accessor = 43;
      return [before, C3.#accessor].join(",");
    }
  }
  console.log(C1.test(), C3.test());
})();

function blk() {
  class Q {
    static a = 1;
    static {
      Q.a += 41;
    }
    static read() {
      return Q.a;
    }
  }
  return Q.read();
}
console.log(blk(), blk());
