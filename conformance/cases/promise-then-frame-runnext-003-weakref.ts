// Once a WeakRef exists, every job boundary clears the kept objects
// (§9.10 ClearKeptObjects), so a frame woken by a `.then` result is
// queued as a job of its own rather than run in the job that settled
// the result: the two jobs keep their boundary. What a cleared target
// reads after the boundary depends on when a collector runs (bun's
// may not have), so the target stays strongly held here; the fixture
// pins the order of every step with the clearing hook installed.
const log: string[] = [];
function inc(v: number): number {
  log.push("inc " + v);
  return v + 1;
}

const target = { name: "t" };
const ref = new WeakRef(target);
log.push("deref " + (ref.deref() === target));

Promise.resolve(0)
  .then(() => log.push("tick 1"))
  .then(() => log.push("tick 2"))
  .then(() => log.push("tick 3"));

const a = await Promise.resolve(1).then(inc);
log.push("main " + a + " " + (ref.deref() === target));
const b = await Promise.resolve(a).then(inc);
log.push("main " + b);
await null;
await null;
console.log(log.join(", "));
