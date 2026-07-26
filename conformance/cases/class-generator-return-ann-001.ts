// Annotating a class generator method — `*g(): Generator<number>` —
// was a hard type error in every member position, sync and async
// alike, while the identical method without the annotation ran.
//
// A generator method parses into two things: a hoisted `function*`
// that becomes the iterator class, and an ordinary forwarder method
// that calls it. The unwrapped yield type T belongs to the first one
// (the desugar builds the class from T and rewrites the decl to answer
// that class). The forwarder was handed T as well — so it declared
// itself to return a number while its body returned the generator
// object: "return type mismatch: function expects Number, got
// ClassRef(__Gen_...)".

class A {
  *sy(): Generator<number> {
    yield 1;
    yield 2;
  }
  static *st(): Generator<string> {
    yield "s";
  }
  async *asy(): AsyncGenerator<number> {
    yield 10;
    yield 20;
  }
  *#priv(): Generator<number> {
    yield 99;
  }
  callPriv(): number {
    let t = 0;
    for (const v of this.#priv()) {
      t = t + v;
    }
    return t;
  }
}

// The object-literal half of the same syntax was never affected — it
// has no forwarder to mis-annotate. It is exercised at top level only:
// an object literal carrying a closure-shaped field is not visible
// from inside a function body (a separate, recorded blocker in the
// lowerer, not in this annotation path).
const lit = {
  *m(): Generator<number> {
    yield 4;
  },
};
for (const v of lit.m()) {
  console.log("lit", v);
}

async function main() {
  const a = new A();
  for (const v of a.sy()) {
    console.log("sy", v);
  }
  for (const v of A.st()) {
    console.log("st", v);
  }
  for await (const v of a.asy()) {
    console.log("asy", v);
  }
  // and held in a variable, so the loop goes through the generic
  // iterator-protocol lane rather than the factory-call desugar
  const held = a.asy();
  for await (const v of held) {
    console.log("held", v);
  }
  console.log("priv", a.callPriv());
}

main();
