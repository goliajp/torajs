// A `Promise<number>` field handed a promise through `any` keeps the
// number it settled with in every reader: `await`, a `.then` handler,
// `Promise.resolve`, and the `Promise.all` / `allSettled` / `race` /
// `any` results (ECMA-262 §27.2.4). Both directions: a field whose
// other writes are fractions given an integer, and a field whose other
// writes are integers given a fraction. An `allSettled` record's
// `value` holds what its element settled with, whichever width that is.

function frac(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}

function ints(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}

class Wide {
  p: Promise<number> = frac(3);
}

class Narrow {
  p: Promise<number> = ints(1);
}

const hf: any = frac(13);
const hi: any = ints(40);

async function main() {
  const w = new Wide();
  w.p = hi;
  console.log("w await", await w.p);
  console.log("w resolve", await Promise.resolve(w.p));
  const wa = await Promise.all([w.p, frac(5)]);
  console.log("w all", wa[0], wa[1], wa[0] + wa[1]);
  const ws = await Promise.allSettled([w.p, frac(5)]);
  console.log("w settled", JSON.stringify(ws));
  for (const x of ws) {
    if (x.status === "fulfilled") console.log("w settled value", x.value + 0.25);
  }
  console.log("w race", await Promise.race([w.p]));
  console.log("w any", await Promise.any([w.p]));

  const n = new Narrow();
  console.log("n first", await n.p);
  n.p = hf;
  console.log("n await", await n.p);
  console.log("n resolve", await Promise.resolve(n.p));
  const na = await Promise.all([n.p, ints(5)]);
  console.log("n all", na[0], na[1], na[0] + na[1]);
  const ns = await Promise.allSettled([n.p, ints(5), Promise.reject(2)]);
  console.log("n settled", JSON.stringify(ns));
  for (const x of ns) {
    if (x.status === "fulfilled") console.log("n settled value", x.value + 0.25);
  }
  console.log("n race", await Promise.race([n.p]));
  console.log("n any", await Promise.any([n.p]));

  const o = { p: ints(2), q: frac(1) };
  o.p = hf;
  o.q = hi;
  const os = await Promise.allSettled([o.p, o.q, Promise.resolve(3)]);
  console.log("o settled", JSON.stringify(os));
  await n.p.then((v: number) => console.log("n then", v));
  await w.p.then((v: number) => console.log("w then", v));
}

main();

Promise.allSettled([ints(8), Promise.resolve(3), frac(3)]).then((a: any) => {
  console.log("top settled", JSON.stringify(a));
});
