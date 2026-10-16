// `(o?.m)(args)` inside a body that gets cloned: a class method reading
// `this` (called on an instance and re-bound with `.call`) and a
// generator called through an `any` alias. A nullish `o` throws a
// TypeError there too; a present one stays the receiver.
class C {
  k = 1;
  run(o: any): string {
    try {
      return String((o?.m)(this.k));
    } catch (e) {
      return e instanceof TypeError ? "TE" : "other";
    }
  }
}
function* gen(o: any) {
  yield 1;
  yield (o?.m)(2);
}
const c = new C();
const src = { m(x: number) { return x * 7; } };
console.log(c.run(src), c.run(null));
const plain: any = { k: 3 };
console.log(C.prototype.run.call(plain, src), C.prototype.run.call(plain, undefined));
for (const v of gen(src)) console.log(v);
const g: any = gen;
try {
  for (const v of g(null)) console.log(v);
} catch (e) {
  console.log("gen", e instanceof TypeError);
}
