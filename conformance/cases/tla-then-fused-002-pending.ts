// `await p.then(g)` with `p` still pending, settled by a later job: the
// reaction running `g` is queued when `p` settles, behind the reaction
// registered on `p` before it, and the continuation one job after.
const log: string[] = [];

function h(v: number): number {
  log.push("h" + v);
  return v + 1;
}

function g(v: number): number {
  log.push("g" + v);
  return v + 10;
}

Promise.resolve()
  .then(() => log.push("t1"))
  .then(() => log.push("t2"))
  .then(() => log.push("t3"))
  .then(() => log.push("t4"))
  .then(() => log.push("t5"));
const p = Promise.resolve(4).then(h).then(h);
p.then((v: number) => log.push("early" + v));
const z = await p.then(g);
log.push("z" + z);
const w = await p.then(g);
log.push("w" + w);
console.log(log.join(","));
