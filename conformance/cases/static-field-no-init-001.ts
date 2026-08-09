// rotation 346 — an uninitialized static field (`static x;` /
// `static #x;`) is a real mutable slot holding undefined: the
// desugar's historical mutable:false carve-out for refcount-typed
// statics plus the Any-slot `__`-sentinel exclusion together made
// every WRITE to it an unknown-ident reject (the 68-case
// rs-static-privatename family).
class C {
  static pub;
  static #priv;
  static #$;
  static setPriv(v) {
    C.#priv = v;
    return C.#priv;
  }
  static dollar(v) {
    C.#$ = v;
    return C.#$;
  }
}
console.log(C.pub);
C.pub = "later";
console.log(C.pub);
console.log(C.setPriv(41), C.dollar("d"));
