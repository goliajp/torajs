// RFC 20260805 D4 — an async generator's steps suspend: `yield v`
// awaits `v` (§27.6.3.8), a yielded rejected promise rejects the step
// and completes the generator, an await inside the body suspends,
// `return v` awaits `v`, and concurrent `next()` calls queue.
const log: string[] = [];
const tick = (tag: string) => Promise.resolve().then(() => { log.push(tag); });
async function* g1() {
  yield 1;
  const v = await Promise.resolve(2);
  yield v;
  yield Promise.resolve(3);
  return await Promise.resolve(4);
}
async function* g2() {
  yield Promise.reject(new Error("no"));
  yield "unreachable";
}
async function* g3() {
  const t = { then(resolve: any, reject: any) { resolve("t"); } };
  yield await t;
  yield t;
}
const order: string[] = [];
async function* g4() { order.push("g4-start"); yield 10; }
async function main() {
  const it = g1();
  // two requests before either settles: they resolve in order
  const p1 = it.next();
  const p2 = it.next();
  const r1 = await p1;
  const r2 = await p2;
  console.log("q", r1.value, r1.done, r2.value, r2.done);
  console.log("step", (await it.next()).value);
  const last = await it.next();
  console.log("return", last.value, last.done);
  console.log("after", (await it.next()).done);
  const it2 = g2();
  try {
    await it2.next();
    console.log("unreachable");
  } catch (e) {
    console.log("rejected", (e as Error).message);
  }
  const r = await it2.next();
  console.log("closed", r.value, r.done);
  const it3 = g3();
  console.log("thenable", (await it3.next()).value, (await it3.next()).value);
  // ordering: a yield settles a tick after the value it awaits
  const it4 = g4();
  const s = it4.next();
  Promise.resolve().then(() => { order.push("tick1"); }).then(() => { order.push("tick2"); });
  s.then(() => { order.push("step-settled"); });
  await tick("a"); await tick("b"); await tick("c");
  console.log("order", order.join(","));
}
main();
