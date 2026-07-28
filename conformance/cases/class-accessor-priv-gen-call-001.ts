// S2.34 — the cls-decl-private-gen-meth t262 shape: a getter answers
// `this.#method` (a private generator), the member CALL runs the
// getter first and dispatches its answer as the callee with the
// instance bound as `this`; trailing comma in the arg list.
var callCount = 0;
class C {
  * #method(a: any, b: any) {
    callCount = callCount + 1;
    yield a;
    yield b;
  }
  get method() {
    return this.#method;
  }
}
var it = new C().method(42, null,);
console.log(it.next().value);
console.log(it.next().value);
console.log(it.next().done);
console.log(callCount);
