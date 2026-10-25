// The async twin of for-head-closure-copies-001: closures a
// `for (let …)` head makes while the loop awaits, in async functions,
// in one that also has a `for await`, and in the module body, against
// a `.then` chain. §14.7.4.4: an update's closure sees the increment
// after it, an initializer's the head's own binding, also through a
// parameter default (§10.2.11).
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(40);

async function update(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; fs.push(() => i), i++) await null;
  return fs.map((h) => h()).join(",");
}

async function updateDefault(): Promise<string> {
  const fs: ((y?: number) => number)[] = [];
  for (let i = 0; i < 3; fs.push((x: number = i) => x), i++) {
    await null;
    log.push("updateDefault " + i);
  }
  return fs.map((h) => h()).join(",");
}

async function init(): Promise<string> {
  const out: number[] = [];
  let last: () => number = () => -1;
  for (let i = 0, g = () => i; i < 3; i++) {
    await null;
    out.push(i, g());
    last = g;
  }
  out.push(last());
  return out.join(",");
}

async function initDefault(): Promise<string> {
  const out: number[] = [];
  for (let i = 0, g = (x: number = i) => x; i < 3; i++) {
    await null;
    out.push(g());
  }
  return out.join(",");
}

async function besideForAwait(): Promise<string> {
  for await (const v of [1]) log.push("forAwait " + v);
  const out: number[] = [];
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => i; i < 3; fs.push(() => i), i++) {
    await null;
    out.push(g());
  }
  return out.join(",") + " " + fs.map((h) => h()).join(",");
}

async function main(): Promise<void> {
  log.push("update " + (await update()));
  log.push("updateDefault " + (await updateDefault()));
  log.push("init " + (await init()));
  log.push("initDefault " + (await initDefault()));
  log.push("besideForAwait " + (await besideForAwait()));
}
await main();

const tops: (() => number)[] = [];
let topLast: () => number = () => -1;
for (let i = 0, g = () => i; i < 3; tops.push(() => i), i++) {
  await null;
  log.push("top " + i + " " + g());
  topLast = g;
}
log.push("top " + tops.map((h) => h()).join(",") + " " + topLast());
for (const line of log) console.log(line);
