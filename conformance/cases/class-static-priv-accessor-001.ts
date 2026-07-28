class C {
  static #x = 5;
  static get #p() { return C.#x }
  static set #p(v: number) { C.#x = v }
  static probe() { C.#p = 9; return C.#p }
}
console.log(C.probe());
