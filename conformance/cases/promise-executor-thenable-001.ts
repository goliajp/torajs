// A `new Promise<T>` whose executor resolves with a thenable — a class
// instance or an object with a `then` method — adopts it and settles
// whatever that `then` calls back with.

class Thenable {
  then(r: (v: any) => void) {
    r("c");
  }
}

class Derived extends Thenable {}

const viaClass = new Promise<number>((r) => r(new Thenable()));
const viaParent = new Promise<number>((r) => r(new Derived()));
const obj = {
  then(r: (v: any) => void) {
    r("o");
  },
};
const viaObject = new Promise<number>((r) => r(obj));

viaClass.then((v) => console.log("then class", typeof v, v));

async function main() {
  const a = await viaClass;
  console.log("await class", typeof a, a);
  const b = await viaParent;
  console.log("await parent", typeof b, b);
  const c = await viaObject;
  console.log("await object", typeof c, c);
}
main();
