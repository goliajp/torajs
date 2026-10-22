// `Promise.resolve(obj)` of typed objects in a program that never gives
// an object a `then`: each fulfils the promise as itself (ECMA-262
// §27.2.1.3.2 step 12), read back through `.then`, `await` and
// `Promise.all` in its own type.

type Pt = { x: number; y: number };

class Acc {
  total = 0;
  add(n: number): Acc {
    this.total = this.total + n;
    return this;
  }
}

const pt: Pt = { x: 1, y: 2 };
Promise.resolve(pt).then((p) => console.log("then", p.x + p.y));
Promise.resolve([1, 2, 3]).then((xs) => console.log("arr", xs.length, xs[2]));
Promise.resolve(new Acc().add(4)).then((a) => console.log("class", a.total));

async function main() {
  const p = await Promise.resolve(pt);
  console.log("await", p.x, p.y);
  const a = await Promise.resolve(new Acc().add(5).add(6));
  console.log("await class", a.total);
  const all = await Promise.all([Promise.resolve(pt), Promise.resolve({ x: 7, y: 8 })]);
  console.log("all", all[0].x, all[1].y);
}
main();
