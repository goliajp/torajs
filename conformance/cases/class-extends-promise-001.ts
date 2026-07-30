// class C extends Promise — exotic-backed promise instances (RFC
// 20260730 blade 2). The instance is a REAL pending Promise cell;
// super(executor) runs §27.2.3.1 against resolving functions bound
// to THIS instance; then/catch/await ride the existing machinery.

// 1. explicit ctor forwarding the executor
class MyPromise extends Promise {
  constructor(ex: any) {
    super(ex);
  }
}
const p = new MyPromise((resolve: any, _reject: any) => {
  resolve(41);
});
console.log(p instanceof Promise, p instanceof MyPromise);
console.log(Object.getPrototypeOf(p) === MyPromise.prototype);
p.then((v: any) => {
  console.log("then", v);
});

// 2. rejection path + executor throw becomes rejection (step 10)
const q = new MyPromise((_resolve: any, reject: any) => {
  reject("nope");
});
q.catch((e: any) => {
  console.log("caught", e);
});
const t = new MyPromise((_resolve: any, _reject: any) => {
  throw "boom";
});
t.catch((e: any) => {
  console.log("threw", e);
});

// 3. class methods over the exotic receiver + await interop
class Tagged extends Promise {
  constructor(ex: any) {
    super(ex);
  }
  label(): string {
    return "T";
  }
}
async function main(): Promise<void> {
  const g = new Tagged((resolve: any, _reject: any) => {
    resolve(7);
  });
  console.log(g.label());
  const v = await g;
  console.log("awaited", v);
  // 4. plain promises keep their answers
  console.log(Promise.resolve(1) instanceof MyPromise);
}
main();
