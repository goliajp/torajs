// RFC 20260915 blade 4 — a sync `for…of` whose body awaits moves into
// the async function's machine (it used to keep the whole function on
// the synchronous lowering): the caller's statement after the call
// runs at the first await ("sync" lands before the second step), and
// the loop's iterator is closed on `break` and on `return` out of the
// body (§7.4.9 — `ret` follows the step that was live), not when the
// iterator reports done.
const log: string[] = [];
async function g(x: number): Promise<number> { log.push("g" + x); return x * 2; }
function mk(): any {
  let i = 0;
  return {
    [Symbol.iterator]() { return this; },
    next() { i++; log.push("next" + i); return { value: i, done: i > 3 }; },
    return() { log.push("ret"); return {}; },
  };
}
async function f(): Promise<number> {
  let sum = 0;
  for (const x of mk()) {
    const v = await g(x);
    sum += v;
    if (x === 2) break;
  }
  log.push("sum" + sum);
  for (const x of mk()) {
    if (x === 1) { await g(x); return sum; }
  }
  return -1;
}
async function done(): Promise<string> {
  let n = 0;
  for (const x of mk()) { n += await g(x); }
  return "n" + n;
}
f().then((v: any) => { log.push("v" + v); console.log("f", log.join(",")); log.length = 0;
  done().then((s: any) => { log.push(s); console.log("done", log.join(",")); }); log.push("sync2"); });
log.push("sync");
