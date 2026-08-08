// RFC 20260809 刀 2 — `await using` (async-dispose hint):
// @@asyncDispose first with @@dispose sync fallback in one scope
// (reverse order), null binding still legal, top-level-await form,
// mixed sync/async scope, and async-reject + body-throw aggregation
// into SuppressedError.
const log: string[] = [];
{
  await using t = { [Symbol.asyncDispose]() { log.push("tla-d"); return Promise.resolve(0); } } as any;
  log.push("tla-body");
}
console.log("tla:", log.join(","));

async function pair(): Promise<void> {
  await using a = { [Symbol.asyncDispose]() { log.push("async-a"); return Promise.resolve(1); } } as any;
  await using b = { [Symbol.dispose]() { log.push("sync-b"); } } as any;
  await using n = null;
  log.push("body");
}
async function mix(): Promise<void> {
  using s = { [Symbol.dispose]() { log.push("mix-sync"); } } as any;
  await using a = { [Symbol.asyncDispose]() { log.push("mix-async"); return Promise.resolve(0); } } as any;
}
async function main(): Promise<void> {
  await pair();
  console.log("pair:", log.join(","));
  await mix();
  console.log("mix:", log.join(","));
  try {
    await using x = { [Symbol.asyncDispose]() { return Promise.reject(new Error("adis")); } } as any;
    throw new Error("abody");
  } catch (e: any) {
    console.log("agg:", e.name, e.error.message, e.suppressed.message);
  }
}
main();
