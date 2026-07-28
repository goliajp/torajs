// RFC 20260729-fn-value-any V2a — a detached generator /
// async-generator method value called bare: the method body never
// observes `this`, so the forwarder is this-free and the call runs
// (constructing the generator object) instead of the
// receiver-undefined TypeError. Covers class decl + class expr, and
// a this-READING generator method keeps its normal receiver path.
class C {
  *m(a: any) {
    yield a;
  }
}
let refC = C.prototype.m;
const g1: any = refC(5);
console.log(g1.next().value);

let D = class {
  async *m(a: any) {
    yield a + 1;
  }
};
let refD = D.prototype.m;
const g2: any = refD(7);
async function main() {
  for await (const v of g2) console.log(v);
  const c = new E();
  const g3: any = c.gen();
  console.log(g3.next().value);
}
class E {
  x: number = 42;
  *gen() {
    yield this.x;
  }
}
main();
