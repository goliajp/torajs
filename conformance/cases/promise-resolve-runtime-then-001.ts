// `Promise.resolve(obj)` of a typed object that gets a `then` only at
// run time, under a key built from parts: PromiseResolve hands it to the
// resolve function, which reads `then` and adopts it (ECMA-262
// §27.2.4.7, §27.2.1.3.2), so every consumer sees what `then` settles
// with, not the object.

type Plain = { a: number };

const parts = ["t", "h", "e", "n"];
let k = "";
for (let i = 0; i < parts.length; i++) {
  k = k + parts[i];
}
const box: Plain = { a: 5 };
const o: any = box;
let calls = 0;
o[k] = (res: (v: number) => void) => {
  calls = calls + 1;
  res(7);
};
const other: Plain = { a: 6 };

Promise.resolve(box).then((v) => console.log("then", typeof v, v.a, String(v)));
Promise.resolve(other).then((v) => console.log("plain", typeof v, v.a));

class Later {
  n = 3;
}
const later = new Later();
(later as any)[k] = (res: (v: string) => void) => res("from class");
Promise.resolve(later).then((v) => console.log("class", typeof v, String(v)));

async function main() {
  const v = await Promise.resolve(box);
  console.log("await", typeof v, String(v));
  const w = await Promise.resolve(box).then((u) => u);
  console.log("chain", typeof w, String(w));
  const xs = await Promise.all([Promise.resolve(box), Promise.resolve(other)]);
  console.log("all", typeof xs[0], String(xs[0]), xs[1].a);
  const y = await Promise.resolve(other);
  console.log("plain await", typeof y, y.a);
  console.log("calls", calls);
}
main();
