// The microtask ticks of a suspending async function against a `.then`
// chain started at the same time: an await of a value that is no
// thenable takes one tick, of a native promise one, of a user thenable
// two; returning a promise settles the result two ticks later, and
// `return await` one.

const log: string[] = [];

function chain(): void {
  Promise.resolve()
    .then(() => { log.push("t1"); })
    .then(() => { log.push("t2"); })
    .then(() => { log.push("t3"); })
    .then(() => { log.push("t4"); });
}

async function plain(): Promise<void> {
  log.push("plain-start");
  await 5;
  log.push("plain-after");
}

async function native(): Promise<void> {
  await Promise.resolve(1);
  log.push("native-after");
}

async function thenable(): Promise<void> {
  const t = { then(r: any) { r(1); } };
  await t;
  log.push("thenable-after");
}

async function retPromise(): Promise<number> {
  await null;
  return Promise.resolve(1);
}

async function retAwait(): Promise<number> {
  await null;
  return await Promise.resolve(2);
}

async function drain(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    await null;
  }
}

async function main(): Promise<void> {
  log.length = 0;
  chain();
  plain().then(() => { log.push("settled"); });
  log.push("sync");
  await drain();
  console.log("plain", log.join(","));

  log.length = 0;
  chain();
  native().then(() => { log.push("settled"); });
  log.push("sync");
  await drain();
  console.log("native", log.join(","));

  log.length = 0;
  chain();
  thenable().then(() => { log.push("settled"); });
  log.push("sync");
  await drain();
  console.log("thenable", log.join(","));

  log.length = 0;
  chain();
  retPromise().then(() => { log.push("settled"); });
  log.push("sync");
  await drain();
  console.log("retPromise", log.join(","));

  log.length = 0;
  chain();
  retAwait().then(() => { log.push("settled"); });
  log.push("sync");
  await drain();
  console.log("retAwait", log.join(","));
}

main();
