// More reads of an element of an array that reached a `Promise<T>[]`
// slot from an `any` value: the element's own `.then`, a destructured
// race winner, `for await` inside an async body, an allSettled record
// over a plain string element, and a typed parameter handed an `any`
// argument. Each element settles as what the `any` side stored
// (§27.7.5.3 Await runs PromiseResolve; §27.2.4.5.3 race).
function mixed(): any { return [Promise.resolve(1), Promise.resolve("x"), "s"]; }
function strs(): any { return ["s"]; }
async function elemThen() {
  const a: Promise<number>[] = mixed();
  await a[1].then((v: number) => console.log("elem then", v + 1));
}
async function destructure() {
  const o: Promise<{ k: number }>[] = strs();
  const { k } = await Promise.race(o);
  console.log("destructured", k);
}
async function forAwaitInBody() {
  const a: Promise<number>[] = mixed();
  for await (const x of a) console.log("body for-await", x, typeof x);
}
async function settled() {
  const a: Promise<number>[] = mixed();
  const rs = await Promise.allSettled(a);
  for (const r of rs) console.log("settled", r.status, r.status === "fulfilled" ? r.value : "-");
}
async function firstOf(xs: Promise<number>[]): Promise<number> {
  return await xs[2];
}
async function main() {
  await elemThen();
  await destructure();
  await forAwaitInBody();
  await settled();
  const v = await firstOf(mixed());
  console.log("param", v, typeof v);
}
main();
