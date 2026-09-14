// RFC 20260915 blade 5 — an async function holding `await using`
// moves into the state machine like any other: the dispose await in
// the scope's finally is a suspend, so the caller's own statements
// run at the first await (the "sync" marks), disposal happens before
// the function's promise settles, a rejecting async dispose and a
// body throw aggregate into SuppressedError, and a `break label`
// past the declaration inside a labeled block resolves.
const log: string[] = [];
async function order(): Promise<void> {
  log.push("a");
  await using x = { [Symbol.asyncDispose]() { log.push("d"); return Promise.resolve(0); } } as any;
  log.push("b");
}
async function ret(): Promise<number> {
  await using x = { [Symbol.asyncDispose]() { log.push("rd"); return Promise.resolve(0); } } as any;
  log.push("rbody");
  return 5;
}
async function agg(): Promise<void> {
  await using x = { [Symbol.asyncDispose]() { return Promise.reject(new Error("adis")); } } as any;
  throw new Error("abody");
}
async function tcf(): Promise<void> {
  try {
    await using a = { [Symbol.asyncDispose]() { log.push("da"); return Promise.resolve(0); } } as any;
    using b = { [Symbol.dispose]() { log.push("db"); } } as any;
    log.push("in");
    throw new Error("x");
  } catch (e: any) {
    log.push("caught:" + e.message);
  } finally {
    log.push("fin");
  }
  log.push("after");
}
async function g(): Promise<number> { log.push("g"); return 1; }
async function loop(): Promise<void> {
  for (let i = 0; i < 2; i++) {
    await using x = { [Symbol.asyncDispose]() { log.push("d" + i); return Promise.resolve(0); } } as any;
    const v = await g();
    log.push("v" + v + "@" + i);
    if (i === 0) continue;
  }
  log.push("done");
}
async function lbl(): Promise<string> {
  var seen = "";
  l: { seen += "body"; if (seen.length > 0) break l; await using _ = null; }
  seen += ",after";
  return seen;
}
async function main(): Promise<void> {
  const p1 = order(); log.push("sync1"); await p1;
  console.log("order", log.join(",")); log.length = 0;
  const p2 = ret(); log.push("sync2"); const v = await p2; log.push("v" + v);
  console.log("ret", log.join(",")); log.length = 0;
  const p3 = agg(); log.push("sync3");
  try { await p3; } catch (e: any) { console.log("agg", e.name, e.error.message, e.suppressed.message); }
  const p4 = tcf(); log.push("sync4"); await p4;
  console.log("tcf", log.join(",")); log.length = 0;
  const p5 = loop(); log.push("sync5"); await p5;
  console.log("loop", log.join(","));
  console.log("lbl", await lbl());
}
main();
