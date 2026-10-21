// A top-level await of an async call whose result an assignment also
// stores (`await (p = f())`, a field, an element): a job that runs
// while the module waits can reach the stored promise and react to it,
// and that reaction runs after the module's continuation, which
// registered first, when the await began (§27.7.5.3).
async function f(v: number): Promise<number> {
  await null;
  await null;
  return v;
}

let p: Promise<number> = Promise.resolve(0);
Promise.resolve().then(() => {
  p.then(() => console.log("b"));
});
const v = await (p = f(1));
console.log("cont", v);

const o: { x: Promise<number> } = { x: Promise.resolve(0) };
Promise.resolve().then(() => {
  o.x.then(() => console.log("b2"));
});
const w = await (o.x = f(2));
console.log("cont2", w);

const a: Promise<number>[] = [Promise.resolve(0)];
Promise.resolve().then(() => {
  a[0].then(() => console.log("b3"));
});
const u = await (a[0] = f(3));
console.log("cont3", u);

// the plain call stays the call's own
Promise.resolve().then(() => console.log("t"));
const x = await f(4);
console.log("cont4", x);
await null;
await null;
console.log("end");
