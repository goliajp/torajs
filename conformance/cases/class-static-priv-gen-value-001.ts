let callCount = 0;
class C {
  static *#method() {
    callCount = callCount + 1;
    yield 9;
  }
  static get method() {
    return this.#method;
  }
}
const it = C.method(1,);
console.log(it.next().value, callCount);
