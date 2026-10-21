// A handler that throws rejects the `.then` result its frame waits
// on: the frame resumes through the general settle and its await
// raises the reason, which rejects the async function's own promise
// (the frame's landing throws, its body has no `try`).
const log: string[] = [];
function boom(v: number): number {
  log.push("boom " + v);
  throw new Error("bad " + v);
}
function ok(v: number): number {
  return v * 2;
}

async function waits(v: number): Promise<number> {
  const w = await Promise.resolve(v).then(boom);
  log.push("unreachable " + w);
  return w;
}
async function fine(v: number): Promise<number> {
  return await Promise.resolve(v).then(ok);
}

waits(1).then(
  (v) => log.push("fulfilled " + v),
  (e) => log.push("rejected " + (e as Error).message),
);
fine(2).then((v) => log.push("fine " + v));
Promise.resolve().then(() => log.push("tick"));
await null;
await null;
await null;
await null;
console.log(log.join(", "));
