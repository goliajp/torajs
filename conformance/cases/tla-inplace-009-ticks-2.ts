// The operand of each top-level await is evaluated before the module
// waits, so the push lands ahead of the tick it races with
// (test262 module-code/top-level-await/top-level-ticks-2).
const expected = [
  "await 1", "tick 1", "await 2", "tick 2",
  "await 3", "tick 3", "await 4", "tick 4",
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

await actual.push("await 1");
await actual.push("await 2");
await actual.push("await 3");
await actual.push("await 4");
