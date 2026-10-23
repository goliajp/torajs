// An async function whose parameters carry no annotation is compiled
// once per argument shape; each copy is the same async function. It
// runs to its first await inside the call, resumes after the caller
// goes on, a throw after the await rejects its promise, and a
// parameter default that hits the TDZ rejects it too.
const log: string[] = [];
async function f(a) {
  log.push("f-start " + a);
  await null;
  log.push("f-after " + a);
  throw new Error("e" + a);
}
async function g(a, b) {
  await null;
  return a + b;
}
const obj = {
  async om(x = y, y) {
    await null;
  },
  async twice(v) {
    const first = await v;
    return first + first;
  },
};
try {
  f(1).catch((e: any) => log.push("caught " + e.message));
  f("s").catch((e: any) => log.push("caught " + e.message));
} catch (e) {
  log.push("sync throw");
}
g(2, 3).then((v) => log.push("g " + v));
g("a", "b").then((v) => log.push("g " + v));
obj.om().then(
  () => log.push("om resolved (wrong)"),
  (e: any) => log.push("om rejected " + (e.constructor === ReferenceError)),
);
obj.twice(Promise.resolve(4)).then((v) => log.push("twice " + v));
obj.twice("x").then((v) => log.push("twice " + v));
log.push("main end");
Promise.resolve()
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => console.log(log.join("\n")));
