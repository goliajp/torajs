// An async function's return wakes what waits on its result: one
// parked frame or one `.then` reaction is queued as the single job
// the settlement triggers, more than one in registration order. The
// ticks against a `.then` chain started at the same time stay those
// of §27.7.5.3 / §27.2.1.8, and a chain of a hundred thousand
// functions each awaiting the next unwinds one job per link, with no
// stack growth.

const log: string[] = [];

function chain(): void {
  Promise.resolve()
    .then(() => { log.push("t1"); })
    .then(() => { log.push("t2"); })
    .then(() => { log.push("t3"); })
    .then(() => { log.push("t4"); })
    .then(() => { log.push("t5"); });
}

async function step(i: number): Promise<number> {
  await null;
  log.push("step" + i);
  return i * 3 + 1;
}

async function now(i: number): Promise<number> {
  log.push("now" + i);
  return i;
}

async function label(i: number): Promise<string> {
  await null;
  return "s" + i;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    await null;
  }
}

async function oneFrame(): Promise<void> {
  const v = await step(1);
  log.push("frame " + v);
}

async function twoFrames(): Promise<void> {
  const p = step(2);
  const a = (async () => { const v = await p; log.push("a " + v); })();
  const v = await p;
  log.push("b " + v);
  await a;
}

async function nested(): Promise<number> {
  const v = await step(3);
  log.push("nested " + v);
  return v + 1;
}

async function outer(): Promise<void> {
  const v = await nested();
  log.push("outer " + v);
}

async function noAwait(): Promise<void> {
  const v = await now(4);
  log.push("noAwait " + v);
}

async function strings(): Promise<void> {
  const s = await label(5);
  log.push("str " + s);
}

async function link(k: number): Promise<number> {
  await null;
  if (k === 0) {
    return 0;
  }
  return (await link(k - 1)) + 1;
}

async function main(): Promise<void> {
  log.length = 0;
  chain();
  oneFrame();
  log.push("sync");
  await settle();
  console.log("frame", log.join(","));

  log.length = 0;
  chain();
  step(6).then((v) => { log.push("then " + v); });
  log.push("sync");
  await settle();
  console.log("then", log.join(","));

  log.length = 0;
  chain();
  twoFrames();
  log.push("sync");
  await settle();
  console.log("two", log.join(","));

  log.length = 0;
  chain();
  outer();
  log.push("sync");
  await settle();
  console.log("nested", log.join(","));

  log.length = 0;
  chain();
  noAwait();
  log.push("sync");
  await settle();
  console.log("noAwait", log.join(","));

  log.length = 0;
  chain();
  strings();
  log.push("sync");
  await settle();
  console.log("strings", log.join(","));

  console.log("deep", await link(100000));
}

main();
