// §27.6.3.8 AsyncGeneratorYield step 5 — an async generator awaits
// what it yields. tr stepped with the promise itself, so a yielded
// rejection RESOLVED the step instead of rejecting it.
//
// One sequential chain on purpose: interleaving independent async
// chains would test microtask ordering, which is a separate ledger.
async function* resolved() { yield Promise.resolve(3) }
async function* rejected() { yield Promise.reject("boom") }
async function* inner() { yield 1; yield 2 }
async function* outer() { yield* inner() }

let error = new Error();
async function* readFile() {
  yield Promise.reject(error);
  yield "unreachable";
}
async function* gen() {
  for await (let line of readFile()) { yield line }
}

// A sync generator yields verbatim (§27.5.3.7) — the value of
// `g().next()` on a yielded promise IS the promise.
function* sync() { yield Promise.resolve(1) }
console.log("sync-verbatim", typeof sync().next().value);

(async () => {
  const r = await resolved().next();
  console.log("resolved", r.value, r.done);

  try {
    await rejected().next();
    console.log("BAD resolved");
  } catch (e) {
    console.log("rejected", e);
  }

  // `yield*` needs no separate arm: delegation desugars into plain
  // yields, and AsyncGeneratorYield is the step each delegated value
  // routes through too.
  const seen = [];
  for await (const v of outer()) seen.push(v);
  console.log("delegated", seen.join(","));

  // The whole shape the rejection family is written as: a rejection
  // yielded by one async generator, consumed by another's `for
  // await`, must reject the outer step AND leave the iterator closed.
  const iter = gen();
  try {
    await iter.next();
    console.log("BAD resolved");
  } catch (rv) {
    console.log("rejects-with-the-same-error", rv === error);
  }
  const after = await iter.next();
  console.log("closed", after.done, after.value);
})();
