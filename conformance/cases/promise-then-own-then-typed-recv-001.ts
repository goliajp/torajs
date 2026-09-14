// §27.2.5.4 is the builtin's body; `p.then(f)` is Invoke(p, "then").
// A cell carrying its own `then` runs the override from a typed
// receiver too — `Promise.resolve(p)` answers `p` itself (§27.2.4.7
// step 2), so the chain that follows is the override's. `await p`
// is PerformPromiseThen and must NOT run it.
const log: string[] = [];
let p: any = Promise.resolve(1);
p.then = function (this: any, onF: any, onR: any) {
  log.push("own then " + typeof onF + "/" + typeof onR);
  return Promise.prototype.then.call(this, onF, onR);
};
Promise.resolve(p).then((v: any) => log.push("v " + v));
Promise.resolve(p).then((v: any) => log.push("v2 " + v), (e: any) => log.push("e " + e));
Promise.resolve(p).then();
// a typed receiver without an override keeps the builtin lane
const q = Promise.resolve(2);
q.then((v: number) => log.push("q " + v));
async function drive() {
  const v = await p;
  log.push("await " + v);
}
drive();
Promise.resolve()
  .then(() => 0)
  .then(() => 0)
  .then(() => 0)
  .then(() => console.log(log.join("\n")));
