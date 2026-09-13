// RFC 20260805 blade 2 — async function VALUES suspend too: an async
// arrow or `async function () {}` expression becomes a hoisted
// generator expression driven by the same promise driver as a
// declaration. Captures ride the generator hoist's own policy: a name
// never reassigned is snapshotted at call time, a reassigned one is
// read by reference. Every function here suspends at its first await,
// so bun's order holds: "sync-end" before any result.
const log: string[] = [];
const push = async (tag: string) => {
  log.push(tag + "-enter");
  await Promise.resolve(0);
  log.push(tag + "-after");
  return tag;
};
const pa = push("a").then((v) => console.log("push", v));
const pb = push("b").then((v) => console.log("push", v));
// an IIFE, the test262 spelling
(async () => {
  console.log("iife-enter");
  await Promise.resolve(0);
  console.log("iife-after");
})();
// a function expression
const fe = async function (x: number) {
  await Promise.resolve(0);
  return x * 2;
};
fe(21).then((v) => console.log("fe", v));
// by-reference capture: the closure sees the write made after the call
let counter = 0;
const bump = async () => {
  await Promise.resolve(0);
  counter = counter + 1;
  return counter;
};
bump().then((v) => console.log("bump", v, counter));
counter = 10;
// a rejection escapes as a rejected promise, never synchronously
const boom = async () => {
  await Promise.resolve(0);
  throw new Error("boom");
};
boom().then(
  () => console.log("bad"),
  (e) => console.log("rejected", e.message)
);
console.log("sync-end");
Promise.all([pa, pb]).then((_r: any[]) => {
  console.log("log", log.join(","));
});
