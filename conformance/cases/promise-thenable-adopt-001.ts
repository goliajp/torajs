// RFC 20260913-thenable-adoption — a user thenable (an object with a
// callable `then`) is ADOPTED wherever a promise is resolved with a
// value: `await`, `Promise.resolve`, a resolving function, a `.then`
// handler's return. §27.2.1.3.2: `then` is read once, the call runs
// in a microtask with a fresh resolving pair, an abrupt `then` read
// rejects, a non-callable `then` is a plain value, and an abrupt call
// rejects unless the thenable already settled.
const log: string[] = [];
const t7 = { then(resolve: any, reject: any) { log.push("t7.then"); resolve(7); } };
const tRej = { then(resolve: any, reject: any) { reject(new Error("no")); } };
const tThrow = { then(resolve: any, reject: any) { throw new Error("boom"); } };
const tSettleThenThrow = { then(resolve: any, reject: any) { resolve(1); throw new Error("late"); } };
const tGetter: any = { get then() { throw new Error("getter"); } };
const tNotCallable = { then: 5 };
// resolves with another thenable — adoption chains
const tNested = { then(resolve: any, reject: any) { resolve(t7); } };
const tPromise = { then(resolve: any, reject: any) { resolve(Promise.resolve("inner")); } };
const err = (e: any) => "rejected:" + (e as Error).message;
const id = (v: any) => v;
async function main() {
  const a1 = await t7;
  console.log("await", a1);
  const a2 = await Promise.resolve(t7);
  console.log("resolve", a2);
  const u: any = { then(resolve: any, reject: any) { resolve(8); } };
  const a3 = await u;
  console.log("await-any", a3);
  const a4 = await Promise.resolve(u);
  console.log("resolve-any", a4);
  const a5 = await new Promise((res: any) => res(t7));
  console.log("ctor", a5);
  const a6 = await Promise.resolve(1).then((_) => t7);
  console.log("handler", a6);
  const a6b = await Promise.resolve(1).then((_) => u);
  console.log("handler-any", a6b);
  const a7 = await Promise.resolve(tRej).then(id, err);
  console.log("rejecting", a7);
  const a8 = await Promise.resolve(tThrow).then(id, err);
  console.log("throwing", a8);
  const a9 = await Promise.resolve(tSettleThenThrow);
  console.log("settle-then-throw", a9);
  const a10 = await Promise.resolve(tGetter).then(id, err);
  console.log("getter", a10);
  const a11 = await Promise.resolve(tNotCallable);
  console.log("not-callable", a11.then);
  const a12 = await tNested;
  console.log("nested", a12);
  const a13 = await tPromise;
  console.log("promise-in-thenable", a13);
  // microtask order: the then call is one job, the settlement another
  const order: string[] = [];
  const p = Promise.resolve({ then(r: any) { order.push("then-call"); r("v"); } });
  Promise.resolve().then(() => { order.push("tick1"); }).then(() => { order.push("tick2"); });
  p.then((v: any) => { order.push("settled:" + v); });
  await Promise.resolve().then(id).then(id).then(id).then(id).then(id);
  console.log("order", order.join(","));
  console.log("log", log.join(","));
}
main();
