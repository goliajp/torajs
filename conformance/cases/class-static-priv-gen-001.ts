class C {
  static *#g() { yield 7; yield 8 }
  static run() {
    const it = C.#g();
    return it.next().value + it.next().value;
  }
}
console.log(C.run());
