// §27.6.3.3 AsyncGeneratorEnqueue: a request made while the generator is
// EXECUTING — its own body calling `iter.next()` before the first
// await — is queued behind the running one, not run. The queue's tail
// is installed before the synchronous drive, so the nested call chains
// on it; it used to re-enter the machine (whose entry had stamped the
// dead sentinel) and answer `done: true`. Second half: a `next()` that
// throws synchronously under `yield*` is the body's own throw
// (§27.6.3.9), so its rejection lands in the same job as the call.
let iter: any;
let order = 0;
let valueIsSet = false;
async function* g() {
  iter.next().then((r: any) => {
    console.log("inner", valueIsSet, ++order, r.value, r.done);
  });
  valueIsSet = true;
  yield 1;
  yield 2;
}
iter = g();
iter.next().then((r: any) => {
  console.log("first", ++order, r.value, r.done);
  iter.next().then((r2: any) => { console.log("third", ++order, r2.value, r2.done); });
});

const reason = { why: "next threw" };
const src = { [Symbol.asyncIterator]() { return { next() { throw reason; } }; } };
async function* h() { yield* src; console.log("not reached"); }
const it = h();
Promise.resolve().then(() => console.log("tick1")).then(() => console.log("tick2"));
it.next().then(() => console.log("fulfilled wrongly"), (e: any) => {
  console.log("rejected with reason:", e === reason);
  it.next().then((r: any) => console.log("after", r.value, r.done));
});
