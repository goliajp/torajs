// A top-level await of an async function declared `Promise<number>`
// whose body returns a promise: the return resolves the result with a
// thenable, which takes two jobs more than returning the number
// (§27.2.1.3.2 NewPromiseResolveThenableJob, then the thenable's own
// reaction), so the module goes on that much later against a `.then`
// chain.
const log: string[] = [];

async function viaPromise(): Promise<number> {
  return Promise.resolve(1);
}

async function viaPromiseLater(): Promise<number> {
  await null;
  return Promise.resolve(2);
}

async function plain(): Promise<number> {
  return 3;
}

async function plainLater(): Promise<number> {
  await null;
  return 4;
}

function chain(tag: string): void {
  Promise.resolve()
    .then(() => log.push(tag + "1"))
    .then(() => log.push(tag + "2"))
    .then(() => log.push(tag + "3"))
    .then(() => log.push(tag + "4"))
    .then(() => log.push(tag + "5"));
}

chain("a");
log.push("r" + (await viaPromise()));
chain("b");
log.push("r" + (await viaPromiseLater()));
chain("c");
log.push("r" + (await plain()));
chain("d");
log.push("r" + (await plainLater()));
await null;
await null;
await null;
await null;
await null;
console.log(log.join(","));
