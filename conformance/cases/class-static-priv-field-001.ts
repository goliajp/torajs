class C {
  static #x = 1;
  static bump() { C.#x = C.#x + 1; return C.#x }
}
console.log(C.bump());
console.log(C.bump());
