// RFC 20260805 D4 — an async generator DECLARED inside an async function
// value (the `asyncTest(async function () { … })` shape): the nested
// declaration is lifted, and the value's body must not be moved a second
// time after the hoist ran — that left a generator expression nobody
// desugared and its `yield` reached the checker.
function run(f: any) { return f().then((v: any) => console.log("done", v), (e: any) => console.log("rej", (e as Error).message)); }
run(async function () {
  async function* g() { yield 1; yield 2; }
  const it = g();
  const a = await it.next();
  const b = await it.next();
  const c = await it.next();
  console.log(typeof g, a.value, a.done, b.value, b.done, c.value, c.done);
  async function* thrower() { throw new Error("boom"); }
  await thrower().next().then(() => console.log("fulfilled"), (e: any) => console.log("caught", e.message));
  const out: number[] = [];
  for await (const v of g()) { out.push(v * 10); }
  return out.join(",");
});
