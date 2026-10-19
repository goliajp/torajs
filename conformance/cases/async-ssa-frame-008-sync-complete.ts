// A suspending async function whose await is not reached completes
// before it returns: its promise is already settled when the caller
// gets it, and the caller's reference to it is its own.

async function maybe(wait: boolean, v: string): Promise<string> {
  if (wait) {
    await null;
  }
  return v;
}

async function main(): Promise<void> {
  const log: string[] = [];
  const p = maybe(false, "now");
  p.then((v: string) => { log.push("then:" + v); });
  log.push("sync");
  const q = maybe(true, "later");
  q.then((v: string) => { log.push("then:" + v); });
  console.log(await p, await q);
  await null;
  console.log(log.join(","));
}

main();
