// RFC 20260915 blade 4 — the generator machine's manual for-of protocol
// closes the iterator (§7.4.9): `break` and `return` out of a
// yield-bearing loop call `return()` (the loop that ran to `done` does
// not), a throw in the body closes before the catch sees it, and a
// generator `return()` injected while suspended inside the loop
// routes through the close too.
function mk(log: string[], tag: string): any {
  let i = 0;
  return {
    [Symbol.iterator]() { return this; },
    next() { i++; log.push(tag + "n" + i); return { value: i, done: i > 3 }; },
    return() { log.push(tag + "ret"); return {}; },
  };
}
function* g1(log: string[]): Generator<number> {
  for (const x of mk(log, "a")) { yield x; if (x === 2) break; }
  for (const x of mk(log, "b")) { yield x; if (x === 1) return 99; }
  return -1;
}
function* g2(log: string[]): Generator<number> {
  try {
    for (const x of mk(log, "c")) { yield x; throw new Error("boom"); }
  } catch (e: any) { log.push("caught:" + e.message); }
}
function* g3(log: string[]): Generator<number> {
  for (const x of mk(log, "d")) { yield x; }
  for (const x of mk(log, "e")) { yield x; }
}
const log: string[] = [];
const it1 = g1(log); let s: any;
while (!(s = it1.next()).done) log.push("v" + s.value);
log.push("ret=" + s.value);
console.log("g1", log.join(",")); log.length = 0;
for (const v of g2(log)) log.push("v" + v);
console.log("g2", log.join(",")); log.length = 0;
const it3 = g3(log); it3.next(); log.push("inj=" + JSON.stringify(it3.return(9)));
log.push("after=" + JSON.stringify(it3.next()));
console.log("g3", log.join(","));
