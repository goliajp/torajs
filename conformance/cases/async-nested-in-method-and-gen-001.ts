// `desugar_async` walks FnDecl bodies at any depth, but a class method
// body is not a FnDecl until `desugar_classes` runs — and that is
// AFTER the async desugar. An async fn declared inside a method kept
// its bare `return`, so an annotated one failed check (`Promise(Number)`
// vs `Number`). A generator body is the same hole through another
// door: `desugar_generators` puts the whole body, nested declarations
// included, inside `__Gen_<name>.next`. `typeof` on the un-awaited call
// result is the discriminator (a leaked bare value answers "number").
class Host {
  run(): void {
    async function inMethod(): Promise<number> {
      return 41 + 1;
    }
    const p = inMethod();
    console.log("method", typeof p);
    p.then((v: number) => console.log("method-v", v));
  }
  static srun(): void {
    async function inStatic() {
      return 7;
    }
    const q = inStatic();
    console.log("static", typeof q);
    q.then((v: any) => console.log("static-v", v));
  }
}
new Host().run();
Host.srun();

function* outerGen(): any {
  async function inGen(): Promise<number> {
    return 20 + 1;
  }
  const p = inGen();
  console.log("gen", typeof p);
  const v = yield p;
  return v * 2;
}
const it = outerGen();
const first = it.next();
first.value.then((v: any) => console.log("gen-v", v, it.next(v).value));
