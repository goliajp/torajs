// `.catch(onErr)` was typed by onErr's return alone, the way
// `.then(onOk)` is. A fulfilled source settles the derived promise with
// its own value (§27.2.5.1: `.catch(f)` is `.then(undefined, f)`), so an
// awaited `.catch` of a fulfilled `Promise<number>` whose handler
// returns a string read the number as a string pointer and crashed.
// The derived value is now the source's or the handler's.
const ok: Promise<number> = Promise.resolve(1);
const bad: Promise<number> = Promise.reject("boom");
const word: Promise<string> = Promise.resolve("w");

const a = await ok.catch((e) => "caught");
console.log("a:", a, typeof a);
const b = await bad.catch((e) => "caught");
console.log("b:", b, typeof b);
const c = await word.catch((e) => 0.5);
console.log("c:", c, typeof c);
const d = await ok.catch((e) => undefined);
console.log("d:", d, d === undefined ? 0 : d + 1);
const e = await bad.catch((e) => undefined);
console.log("e:", e, e === undefined);
const f = await ok.catch((e) => 9);
console.log("f:", f + 1);
const g = await ok.catch((e) => {
  throw new Error("not run");
});
console.log("g:", g);

ok.catch((e) => "x")
  .then((v) => (typeof v === "number" ? v * 100 : v))
  .then((w) => console.log("chain:", w));

async function inBody(p: Promise<number>, tag: string): Promise<void> {
  const r = await p.catch((err) => "body " + err);
  console.log(tag, r, typeof r);
  const n = await p.catch((err) => -1);
  console.log(tag, "same:", n * 2);
}
await inBody(ok, "ok:");
await inBody(bad, "bad:");
