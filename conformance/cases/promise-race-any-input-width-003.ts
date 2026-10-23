// An element of an array that reached a `Promise<T>[]` slot from an
// `any` value is whatever the `any` side stored: awaiting it runs
// PromiseResolve on it (§27.7.5.3), so a string element settles as the
// string, whether or not a combinator also read the array. A
// Promise.race / Promise.any winner over such an array may be an object
// or an array whatever the element type says (§27.2.4.5.3, §27.2.4.3.1).
function strFirst(): any { return ["s", Promise.resolve(1)]; }
function strOnly(): any { return ["s"]; }
function objOnly(): any { return [{ k: 7 }]; }
function objPair(): any { return ["s", Promise.resolve({ k: 2 })]; }
const top: Promise<number>[] = strFirst();
console.log("top elem", await top[0], (await top[1]) + 1);
for await (const x of top) console.log("top for-await", x, typeof x);
async function elems() {
  const a: Promise<number>[] = strFirst();
  console.log("elem", await a[0]);
  const x = await a[0];
  console.log("elem binding", x, typeof x);
  const y: number = await a[1];
  console.log("elem typed", y + 1);
  console.log("elem plus", (await a[0]) + 1, (await a[1]) + 1);
  const p = a[0];
  console.log("elem alias", await p);
}
async function elemsThenRace() {
  const a: Promise<number>[] = strFirst();
  console.log("before race", await a[0]);
  console.log("race", await Promise.race(a));
}
async function shapes() {
  const o: Promise<{ k: number }>[] = strOnly();
  const w = await Promise.race(o);
  console.log("race obj slot", w, w.k);
  const r: Promise<number[]>[] = strOnly();
  const z = await Promise.any(r);
  console.log("any arr slot", z.length, z[0]);
  const real: Promise<{ k: number }>[] = objOnly();
  const v = await Promise.race(real);
  console.log("race real obj", v.k + 1);
  const pair: Promise<{ k: number }>[] = objPair();
  const e = await pair[1];
  console.log("elem obj", e.k + 1);
  const s = await pair[0];
  console.log("elem obj slot", s, s.k);
}
function thenReader() {
  const a: Promise<number>[] = strOnly();
  Promise.race(a).then((v: number) => console.log("then", v + 1));
}
await elems();
await elemsThenRace();
await shapes();
thenReader();
