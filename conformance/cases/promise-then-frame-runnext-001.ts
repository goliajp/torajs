// A frame that is the only waiter of a `.then` result resumes in the
// job that settled the result when nothing else is queued, and is
// queued like any reaction otherwise. Either way the order of every
// observable step is the one §27.2.2.1 / §27.7.5.3 give: here the
// module and an async function each await a `.then` result while a
// separate `.then` chain ticks along.
const log: string[] = [];
function inc(v: number): number {
  log.push("inc " + v);
  return v + 1;
}

Promise.resolve(0)
  .then(() => log.push("tick 1"))
  .then(() => log.push("tick 2"))
  .then(() => log.push("tick 3"))
  .then(() => log.push("tick 4"))
  .then(() => log.push("tick 5"));

async function inner(v: number): Promise<number> {
  const w = await Promise.resolve(v).then(inc);
  log.push("inner " + w);
  return w;
}

const a = await Promise.resolve(10).then(inc);
log.push("main " + a);
const b = await inner(20);
log.push("main " + b);
await null;
await null;
await null;
console.log(log.join(", "));

let total = 0;
for (let i = 0; i < 1000; i = i + 1) {
  total = total + (await Promise.resolve(i).then(inc));
}
console.log(total, log.length);
