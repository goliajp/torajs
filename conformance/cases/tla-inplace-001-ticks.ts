// Each top-level `await <non-thenable>` queues the module's
// continuation as exactly one job (§16.2.1.5.3, §27.7.5.3), so the
// continuations interleave with a `.then` chain started before them,
// one tick each (test262 module-code/top-level-await/top-level-ticks).
// The module body runs the jobs queued ahead of each continuation
// itself and goes on in place.
const expected = [
  "tick 1", "await 1", "tick 2", "await 2",
  "tick 3", "await 3", "tick 4", "await 4",
];
const actual: string[] = [];

Promise.resolve(0)
  .then(() => actual.push("tick 1"))
  .then(() => actual.push("tick 2"))
  .then(() => actual.push("tick 3"))
  .then(() => actual.push("tick 4"))
  .then(() => {
    console.log(actual.join(", "));
    console.log(actual.join() === expected.join());
  });

await 1; actual.push("await 1");
await 2; actual.push("await 2");
await 3; actual.push("await 3");
await 4; actual.push("await 4");
console.log("body done", actual.length);
