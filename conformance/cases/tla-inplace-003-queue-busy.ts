// The awaited cell settles while another job is still queued behind
// the settling one: the module's continuation is a job of its own and
// runs after that one (§27.7.5.3 Await queues it at the tail), not in
// the job that settled the awaited cell.
const log: string[] = [];
function a(): void {
  log.push("a");
}
function b(): void {
  log.push("b");
}
function f(v: number): number {
  log.push("f");
  return v + 1;
}

Promise.resolve().then(a);
const d = Promise.resolve(1).then(f);
Promise.resolve().then(b);
const v = await d;
log.push("cont " + v);
console.log(log.join(","));

// the same with the queue busy only behind the settling job
log.length = 0;
const e = Promise.resolve(5).then(f);
Promise.resolve().then(a).then(b);
const w = await e;
log.push("cont " + w);
await null;
await null;
console.log(log.join(","));
