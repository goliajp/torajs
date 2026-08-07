// Rotation 326 — `void <ident>` over a borrow. The §13.5.2 desugar
// turns `void D` into a Sequence whose discarded left was dropped
// unconditionally by type shape: an ident-bound borrow (here the
// class binding, whose stake belongs to the class registry) was
// released once per mention — one line underflowed the class
// object's rc (census: class-static-001-blocks, one hit per class).
class D {
  static tag: string = "D"
}
void D
console.log(typeof (void D))
console.log(D.tag)

// owned left keeps its release: a call result discarded through
// `void` must still be freed — the leak side of the same contract.
function mk(): number[] {
  return [1, 2, 3]
}
void mk()
console.log("done")
