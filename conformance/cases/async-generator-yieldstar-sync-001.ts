// RFC 20260805 D4 — `yield*` and a yield-bearing `for await` over a
// SYNC iterable inside an async generator take the Async-from-Sync
// posture (§27.1.4.4): each element is awaited (a promise-valued one
// unwraps), a throwing `next()` becomes a rejection at the await, and
// an async iterable's `next()` is called as is.
const syncIter = {
  [Symbol.iterator]() {
    let i = 0;
    return { next() { i++; return i <= 2 ? { value: Promise.resolve(i * 10), done: false } : { value: undefined, done: true }; } };
  },
};
async function* viaYieldStar() { yield* syncIter; }
async function* viaForAwait() { for await (const v of syncIter) { yield v + 1; } }
function* thrower(): Generator<number> { throw new Error("boom"); }
async function* delegatesThrow() { yield* thrower(); }
async function* inner() { yield "a"; yield "b"; }
async function* viaAsync() { yield* inner(); }
async function main() {
  const a: any[] = [];
  for await (const v of viaYieldStar()) { a.push(v); }
  const b: any[] = [];
  for await (const v of viaForAwait()) { b.push(v); }
  const c: any[] = [];
  for await (const v of viaAsync()) { c.push(v); }
  console.log("yield*", a.join(","), "for-await", b.join(","), "async", c.join(","));
  const order: string[] = [];
  const it = delegatesThrow();
  const p = it.next().then(() => order.push("resolved"), (e) => order.push("rejected:" + (e as Error).message));
  Promise.resolve().then(() => order.push("tick1")).then(() => order.push("tick2")).then(() => order.push("tick3"));
  await p;
  await Promise.resolve().then(() => 0).then(() => 0).then(() => 0);
  console.log("order", order.join(","));
}
main();
