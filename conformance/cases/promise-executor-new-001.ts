// rotation 234 — `new Promise(executor)` (§27.2.3.1). Desugars to a
// synthesized helper over `Promise.withResolvers()` (§27.2.4.8): the
// runtime already owned the pending cell and its settle pair; the
// constructor form just runs the executor synchronously against them
// and turns an executor throw into a rejection.

// The executor runs synchronously; handlers run as microtasks.
console.log("A");
const p1 = new Promise((res, rej) => {
  console.log("in-executor");
  res(42);
});
p1.then((v) => {
  console.log("ok:", v);
});
console.log("B");

// Rejection through the second settle fn.
const p2 = new Promise((res, rej) => {
  rej("nope");
});
p2.catch((e) => {
  console.log("err:", e);
});

// §27.2.3.1 step 10 — an executor throw rejects the promise.
const p3 = new Promise((res, rej) => {
  throw new Error("thrown");
});
p3.catch((e) => {
  console.log("threw:", e.message);
});

// The settle fn escapes the executor and fires later — the
// withResolvers shape underneath makes this the natural case.
let saved: any = null;
const p4 = new Promise((r) => {
  saved = r;
});
p4.then((v) => {
  console.log("late:", v);
});
saved(9);

// await over a constructed promise.
async function main() {
  const v = await new Promise((res) => {
    res(7);
  });
  console.log("awaited:", v);
}
main();
