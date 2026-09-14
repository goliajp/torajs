// RFC 20260915 blade 4 — `for await` inside an async function's machine:
// each step is a suspend (the caller's "sync" lands after the first
// `next()` and before the first body), an async iterator is closed on
// `break` (§7.4.11, awaited), a sync iterable's Promise elements
// unwrap through the Async-from-Sync posture, a throw in the body
// closes the iterator and still reaches the catch, a labeled
// `continue` out of the loop closes it, and `return` from inside the
// loop in a nested async arrow answers the element.
const log: string[] = [];
function mkAsync(tag: string): any {
  let i = 0;
  return {
    [Symbol.asyncIterator]() { return this; },
    next() { i++; log.push(tag + "next" + i); return Promise.resolve({ value: i, done: i > 3 }); },
    return() { log.push(tag + "ret"); return Promise.resolve({ done: true }); },
  };
}
async function f(): Promise<number> {
  let sum = 0;
  for await (const x of mkAsync("a")) {
    log.push("body" + x);
    sum += x;
    if (x === 2) break;
  }
  log.push("sum" + sum);
  for await (const x of [Promise.resolve(10), 20]) { log.push("sync-src" + x); }
  try {
    for await (const x of mkAsync("b")) { throw new Error("boom" + x); }
  } catch (e: any) { log.push("caught:" + e.message); }
  outer: for (const a of [1, 2]) {
    for await (const b of mkAsync("c")) {
      log.push(a + ":" + b);
      if (b === 1) continue outer;
    }
    log.push("unreached");
  }
  const r = await (async () => { for await (const z of [Promise.resolve("z")]) { return z; } return "none"; })();
  log.push("r=" + r);
  return sum;
}
const p = f(); log.push("sync");
p.then((v: any) => { log.push("v" + v); console.log("p", log.join(",")); });
