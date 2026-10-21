// A top-level await of a pending `.then` result goes on after every
// job queued ahead of the settlement that reaches it (§27.2.1.8 runs
// reactions in registration order): the module waits for `d`, whose
// handler runs before `g`'s, and the module itself after both. With
// another reaction on the same cell the two run in the order they
// registered.
const log: string[] = [];
function f(v: number): number {
  log.push("f" + v);
  return v * 10;
}
function g(v: number): number {
  log.push("g" + v);
  return v;
}
function h(v: number): number {
  log.push("h" + v);
  return v;
}

const d = Promise.resolve(1).then(f);
Promise.resolve(2).then(g);
const v = await d;
log.push("cont " + v);
console.log(log.join(","));

log.length = 0;
const d2 = Promise.resolve(3).then(f);
d2.then(h);
const v2 = await d2;
log.push("cont " + v2);
console.log(log.join(","));

log.length = 0;
const d3 = Promise.resolve(4).then(f);
const v3 = await d3;
d3.then(h);
log.push("cont " + v3);
await null;
console.log(log.join(","));
