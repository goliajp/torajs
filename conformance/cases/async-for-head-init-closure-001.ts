// The async twin of for-head-init-closure-001: a `for` whose
// initializer makes a function that reads none of the head's `let`
// bindings, its body awaiting, against a `.then` chain. Each
// iteration has its own copy of the bindings (§14.7.4.4), and the
// awaits keep their order.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(16);

async function plain(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => 0; i < 3; i++) {
    fs.push(() => i);
    await null;
    log.push("plain " + i);
  }
  return fs.map((h) => h()).join(",");
}

async function callsInitFn(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => 10; i < 3; i++) {
    fs.push(() => i + g());
    await null;
  }
  return fs.map((h) => h()).join(",");
}

async function mutatesInBody(): Promise<string> {
  const fs: (() => string)[] = [];
  for (let i = 0, s = "a", g = (x: string) => x + "!"; i < 3; i++) {
    s = s + i;
    await null;
    fs.push(() => g(s) + i);
  }
  return fs.map((h) => h()).join(",");
}

async function main(): Promise<void> {
  log.push("plain " + (await plain()));
  log.push("callsInitFn " + (await callsInitFn()));
  log.push("mutatesInBody " + (await mutatesInBody()));
}
await main();
for (const line of log) console.log(line);
