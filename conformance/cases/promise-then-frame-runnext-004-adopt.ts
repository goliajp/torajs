// A handler that returns a promise: the `.then` result adopts it
// (§27.2.1.3.2 resolve with a thenable, NewPromiseResolveThenableJob,
// two more jobs), so a frame waiting on the result is not resumed in
// the handler's job; the ticks of a parallel chain show the delay.
const log: string[] = [];
function later(v: number): Promise<number> {
  log.push("later " + v);
  return Promise.resolve(v + 100);
}
function plain(v: number): number {
  log.push("plain " + v);
  return v + 1;
}

Promise.resolve(0)
  .then(() => log.push("tick 1"))
  .then(() => log.push("tick 2"))
  .then(() => log.push("tick 3"))
  .then(() => log.push("tick 4"))
  .then(() => log.push("tick 5"))
  .then(() => log.push("tick 6"));

const a = await Promise.resolve(1).then(later);
log.push("adopted " + a);
const b = await Promise.resolve(2).then(plain);
log.push("plain " + b);
await null;
await null;
await null;
await null;
console.log(log.join(", "));
