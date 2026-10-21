// The top level awaits the result of an async function that awaits
// itself: the continuation runs one job after the function's return,
// behind the `.then` links queued meanwhile.
const log: string[] = [];

async function step(i: number): Promise<number> {
  log.push("s" + i);
  await null;
  log.push("s" + i + "b");
  return i * 2;
}

async function now(i: number): Promise<number> {
  log.push("n" + i);
  return i + 100;
}

Promise.resolve()
  .then(() => log.push("a"))
  .then(() => log.push("b"))
  .then(() => log.push("c"))
  .then(() => log.push("d"))
  .then(() => log.push("e"))
  .then(() => log.push("f"));
const r1 = await step(1);
log.push("r" + r1);
const r2 = await now(2);
log.push("r" + r2);
const r3 = await step(3);
log.push("r" + r3);
console.log(log.join(","));
