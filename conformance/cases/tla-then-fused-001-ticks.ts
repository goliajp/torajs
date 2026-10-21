// `await p.then(f)` at the top level, against a `.then` chain: the
// reaction running `f` and the await's continuation are one job each,
// in queue order with the chain's links.
const log: string[] = [];

function f(v: number): number {
  log.push("f" + v);
  return v * 2;
}

Promise.resolve()
  .then(() => log.push("a1"))
  .then(() => log.push("a2"))
  .then(() => log.push("a3"))
  .then(() => log.push("a4"))
  .then(() => log.push("a5"));
const x = await Promise.resolve(1).then(f);
log.push("x" + x);
const y = await Promise.resolve(2).then(f);
log.push("y" + y);
console.log(log.join(","));
