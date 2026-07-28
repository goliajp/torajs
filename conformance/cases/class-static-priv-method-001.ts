class C {
  static #m(x: number) { return x + 1 }
  static call() { return C.#m(41) }
}
console.log(C.call());
