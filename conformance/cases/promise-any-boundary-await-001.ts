// A `Promise<T>` annotation over a promise settled with an `any` value
// has no runtime meaning (§27.2.4.7 settles the value as it is): an
// `await` of a binding or a call declared that way, of a handler chained
// on it, or of a combinator over it reads the string that was settled.

function src(x: any): Promise<number> {
  return Promise.resolve(x);
}

async function main() {
  const x: any = JSON.parse('"str"');
  const a = await src(x).then((v: number) => v + 1);
  console.log("then", typeof a, a);
  const b = await src(x).then((v: number) => ({ a: v }));
  console.log("object", typeof b.a, b.a);
  const c = await Promise.all([src(x).then((v: number) => v + 1)]);
  console.log("all", typeof c[0], c[0]);
  const p: Promise<number> = src(x).then((v: number): number => v + 1);
  const d = await p;
  console.log("binding of then", typeof d, d);
  const q: Promise<number> = Promise.resolve(x);
  const e = await q;
  console.log("binding", typeof e, e);
  const y: any = 7;
  const f = await src(y).then((v: number) => v + 1);
  console.log("number", typeof f, f);
}
main();
