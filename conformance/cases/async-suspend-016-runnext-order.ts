// A return that wakes the one waiter on its result fulfils the result
// in place and queues the waiter's job at the tail. The ticks stay those of
// §27.2.1.8 when that job queues more jobs, when two returns land in
// adjacent jobs, when several functions wait on one another in a
// diamond, and with a WeakRef alive (the job boundary between the
// return and its waiter still clears the kept objects).

const log: string[] = [];

async function leaf(i: number): Promise<number> {
  await null;
  log.push("leaf" + i);
  return i;
}

async function fanOut(): Promise<number> {
  const v = await leaf(1);
  log.push("fan " + v);
  Promise.resolve().then(() => { log.push("q1"); }).then(() => { log.push("q2"); });
  queueMicrotask(() => { log.push("m1"); });
  return v + 10;
}

async function afterFan(): Promise<void> {
  const v = await fanOut();
  log.push("after " + v);
}

async function pair(): Promise<void> {
  const a = leaf(2);
  const b = leaf(3);
  log.push("pair " + (await a) + " " + (await b));
}

async function left(p: Promise<number>): Promise<number> {
  return (await p) + 100;
}

async function right(p: Promise<number>): Promise<number> {
  return (await p) + 200;
}

async function diamond(): Promise<void> {
  const top = leaf(4);
  const l = left(top);
  const r = right(top);
  log.push("diamond " + (await l) + " " + (await r));
}

async function chain(k: number): Promise<number> {
  if (k === 0) {
    return await leaf(5);
  }
  const v = await chain(k - 1);
  log.push("c" + k);
  return v + k;
}

const target = { name: "kept" };
const ref = new WeakRef(target);

async function weak(): Promise<void> {
  const v = await chain(3);
  log.push("weak " + v + " " + (ref.deref() as any).name);
}

async function settle(): Promise<void> {
  for (let i = 0; i < 16; i++) {
    await null;
  }
}

function ticks(): void {
  Promise.resolve()
    .then(() => { log.push("t1"); })
    .then(() => { log.push("t2"); })
    .then(() => { log.push("t3"); })
    .then(() => { log.push("t4"); });
}

async function main(): Promise<void> {
  log.length = 0;
  afterFan();
  await settle();
  console.log("fan", log.join(","));

  log.length = 0;
  ticks();
  afterFan();
  await settle();
  console.log("fan+ticks", log.join(","));

  log.length = 0;
  pair();
  await settle();
  console.log("pair", log.join(","));

  log.length = 0;
  diamond();
  await settle();
  console.log("diamond", log.join(","));

  log.length = 0;
  weak();
  await settle();
  console.log("weak", log.join(","));

  log.length = 0;
  ticks();
  weak();
  await settle();
  console.log("weak+ticks", log.join(","));
}

main();
