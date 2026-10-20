// A promise of a fraction handed in through `any` keeps its fraction
// wherever a `Promise<number>` receives it (ECMA-262 §27.2.5.4,
// §27.7.5.3): an `as` assertion (awaited in place, bound, inside
// `Promise.all`), a Set's `add`, a Map's `set`, an indexed store, a
// class method's parameter (typed and through an `any` receiver), and
// a handler chained on a promise bound from `any`. Each section has
// helpers of its own and reads an integer through the same value
// point first.

function frac1(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints1(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h1: any = frac1(13);
async function s1() {
  console.log("as in place", await (ints1(1) as Promise<number>), await (h1 as Promise<number>));
  const a = ints1(1) as Promise<number>;
  const b = h1 as Promise<number>;
  console.log("as bound", await a, await b);
  const all = await Promise.all([ints1(2), h1 as Promise<number>]);
  console.log("as all", all[0], all[1]);
}

function frac2(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints2(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h2: any = frac2(13);
async function s2() {
  const s = new Set<Promise<number>>();
  s.add(ints2(1));
  s.add(h2);
  const got: number[] = [];
  for (const p of s) {
    const v: number = await p;
    got.push(v);
  }
  console.log("set add", got[0], got[1]);
  const m = new Map<string, Promise<number>>();
  m.set("i", ints2(1));
  m.set("f", h2);
  const pi: Promise<number> = m.get("i");
  const pf: Promise<number> = m.get("f");
  console.log("map set", await pi, await pf);
  const xs: Promise<number>[] = [];
  xs[0] = ints2(1);
  xs[1] = h2;
  console.log("index store", await xs[0], await xs[1]);
}

function frac3(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints3(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h3: any = frac3(13);
class Reader3 {
  async m(p: Promise<number>): Promise<number> {
    return await p;
  }
}
async function s3() {
  const r = new Reader3();
  console.log("method", await r.m(ints3(1)), await r.m(h3));
  const d: any = new Reader3();
  console.log("any receiver", await r.m(ints3(2)), await d.m(h3));
}

function frac4(n: number): Promise<number> {
  return new Promise((r) => r(n / 2));
}
function ints4(n: number): Promise<number> {
  return Promise.resolve(n + 1);
}
const h4: any = frac4(13);
async function s4() {
  const p: Promise<number> = ints4(1);
  const q: Promise<number> = h4;
  const x = await p.then((v) => v * 2);
  const y = await q.then((v) => v * 2);
  console.log("then", x, y);
}

async function main() {
  await s1();
  await s2();
  await s3();
  await s4();
}
main();
