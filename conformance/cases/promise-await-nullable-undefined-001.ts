// An awaited `Promise<T | undefined>` (or `T | null`) read its settled
// value as if the union were `T`: an undefined number came back as
// NaN, a null one as NaN, an undefined string / object / array as
// null, and a `Promise.all` of such promises as raw box bits. The read
// now takes the slot a binding of the union gets, and the settled
// cell's stamp keeps undefined and null apart.
type Pt = { x: number };

function maybe(i: number): Promise<number | undefined> {
  return Promise.resolve(i % 2 === 0 ? i : undefined);
}
async function numOrU(i: number): Promise<number | undefined> {
  if (i > 0) return i * 1.5;
  return undefined;
}
async function numOrNull(i: number): Promise<number | null> {
  return i > 0 ? i + 0.25 : null;
}
async function strOrU(i: number): Promise<string | undefined> {
  return i > 0 ? "s" + i : undefined;
}
async function strOrNull(i: number): Promise<string | null> {
  return i > 0 ? "t" + i : null;
}
async function ptOrU(i: number): Promise<Pt | undefined> {
  return i > 0 ? { x: i } : undefined;
}
async function arrOrU(i: number): Promise<number[] | undefined> {
  return i > 0 ? [i, i + 1] : undefined;
}
async function boolOrU(i: number): Promise<boolean | undefined> {
  return i > 0 ? i > 1 : undefined;
}

async function scalars(): Promise<void> {
  const a = await maybe(2);
  const b = await maybe(3);
  console.log("maybe:", a, b, b === undefined, typeof b);
  const c = await numOrU(3);
  const d = await numOrU(0);
  console.log("numOrU:", c, d, d === undefined, c === undefined ? -1 : c * 2);
  const e = await numOrNull(1);
  const f = await numOrNull(0);
  console.log("numOrNull:", e, f, f === null, e === null ? -1 : e + 1);
  console.log("boolOrU:", await boolOrU(2), await boolOrU(1), await boolOrU(0));
  const p: Promise<number | undefined> = Promise.resolve(undefined);
  console.log("annotated:", await p);
  const nested = await await Promise.resolve(maybe(5));
  console.log("nested:", nested);
  let sum = 0;
  let misses = 0;
  for (let i = 0; i < 6; i++) {
    const v = await maybe(i);
    if (v === undefined) misses++;
    else sum += v;
  }
  console.log("loop:", sum, misses);
}

async function pointers(): Promise<void> {
  const s1 = await strOrU(1);
  const s0 = await strOrU(0);
  console.log("strOrU:", s1, s0, s0 === undefined, typeof s0);
  console.log("strOrU length:", s1 === undefined ? -1 : s1.length);
  console.log("strOrNull:", await strOrNull(0), await strOrNull(2));
  const o1 = await ptOrU(4);
  const o0 = await ptOrU(0);
  console.log("ptOrU:", o0, o0 === undefined, o1 === undefined ? -1 : o1.x);
  const r1 = await arrOrU(1);
  const r0 = await arrOrU(0);
  console.log("arrOrU:", r0, r0 === undefined, r1 === undefined ? -1 : r1.length);
}

async function combined(): Promise<void> {
  const all = await Promise.all([maybe(1), maybe(4), numOrU(2), numOrU(0)]);
  console.log("all:", all[0], all[1], all[2], all[3], all.length);
  const strs = await Promise.all([strOrU(0), strOrU(7)]);
  console.log("all str:", strs[0], strs[1]);
}

await scalars();
await pointers();
await combined();
maybe(9).then((v) => console.log("then:", v));
await numOrU(0).then((v) => console.log("then numOrU:", v));
const top = await maybe(11);
console.log("top:", top, top === undefined);
const top2 = await numOrU(2);
console.log("top2:", top2);
