// A promise handler held in a slot rather than named by a declaration
// sees the settled value at its real width, and hands back its own.
// Every local of an async body is such a slot (the state machine keeps
// it on the generator object), so `await p.then(cb)` with a local `cb`
// is the common case. Inputs are computed at run time (argv length is
// 2 on both sides).

const half = process.argv.length / 4;
const big = process.argv.length * 0 + 2 ** 53 + 2;

async function thenLocal(): Promise<void> {
  const cb = (v: number): number => v + 1;
  console.log("then", await Promise.resolve(half).then(cb));
}

async function catchLocal(): Promise<void> {
  const cb = (_e: any): number => half + 0.25;
  console.log("catch", await Promise.reject(new Error("x")).catch(cb));
}

async function finallyLocal(): Promise<void> {
  const cb = (): void => {};
  console.log("finally", await Promise.resolve(half).finally(cb));
}

async function allLocal(): Promise<void> {
  const cb = (v: number): number => v * 3;
  const r = await Promise.all([Promise.resolve(half).then(cb), Promise.resolve(2)]);
  console.log("all", r[0], r[1]);
}

async function bigLocal(): Promise<void> {
  const cb = (v: number): number => v * 0 + big;
  console.log("big", await Promise.resolve(1).then(cb));
}

async function nanLocal(): Promise<void> {
  const cb = (v: number): number => v * NaN;
  console.log("nan", await Promise.resolve(2).then(cb));
}

async function twoLocal(): Promise<void> {
  const ok = (v: number): number => v / 4;
  const bad = (_e: any): number => -1;
  console.log("two", await Promise.resolve(3).then(ok, bad));
}

async function laterLocal(): Promise<void> {
  const cb = (v: number): number => v + 1;
  const p = Promise.resolve(half).then(cb);
  await 0;
  console.log("later", await p);
}

async function returnsHalf(): Promise<number> {
  return half;
}

async function awaitReturn(): Promise<void> {
  console.log("return", await returnsHalf());
}

function viaParam(f: (v: number) => number): Promise<number> {
  return Promise.resolve(half).then(f);
}

async function paramHandler(): Promise<void> {
  console.log("param", await viaParam((v: number): number => v + 2));
}

function fieldHandler(): Promise<void> {
  const o: any = {};
  o.cb = (v: number): number => v + 1;
  return Promise.resolve(half)
    .then(o.cb)
    .then((r: any): void => {
      console.log("field", r);
    });
}

async function rejectedTwo(): Promise<void> {
  const ok = (v: number): number => v / 4;
  const bad = (_e: any): number => half;
  console.log("rejected", await Promise.reject(new Error("x")).then(ok, bad));
}

// An await the async desugar leaves inline reads the settled slot
// raw, so the handler's faces have to match the value point both
// ways: an integer handed back into a fractional chain, and a
// fractional parameter (widened by its other call) fed an integer.
type Holder = { cb: (v: number) => number };
const seven: Holder = { cb: (_v: number): number => 7 };
const bump: Holder = { cb: (v: number): number => v + 1 };

function* gen(): Generator<number> {
  const step = (v: number): number => v + 0.5;
  yield half;
  yield step(half);
}

async function main(): Promise<void> {
  await thenLocal();
  await catchLocal();
  await finallyLocal();
  await allLocal();
  await bigLocal();
  await nanLocal();
  await twoLocal();
  await laterLocal();
  await awaitReturn();
  await paramHandler();
  await fieldHandler();
  await rejectedTwo();
  for (const v of gen()) console.log("gen", v);
}

await main();
console.log("bump direct", bump.cb(half));
console.log("int into frac", await Promise.resolve(half).then(seven.cb));
console.log("frac param", await Promise.resolve(process.argv.length).then(bump.cb));
