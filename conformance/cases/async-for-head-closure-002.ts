// A `for` loop in an async body, against a `.then` chain. A function
// the update makes that reads the head's `let` binding sees the copy
// of the iteration it runs in, the update's own later change included
// (§14.7.4.4); one the initializer or the update makes that reads no
// `let` of the head leaves the body's closures their own copies; a
// `const` head has none (§14.7.4.2), a parameter default over it
// included.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(24);

async function updateNoLet(): Promise<string> {
  const fs: (() => string)[] = [];
  const tag = "u";
  for (let i = 0; i < 3; i++, fs.push(() => tag)) {
    fs.push(() => tag + i);
    await null;
  }
  return fs.map((g) => g()).join(",");
}

async function updateBody(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; fs.push(() => i), i++) {
    await null;
    log.push("updateBody " + i);
  }
  return fs.map((g) => g()).join(",");
}

async function initReadsNoLet(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0, g = () => 0; i < 3; i++) {
    fs.push(() => i);
    await null;
  }
  return fs.map((h) => h()).join(",");
}

async function constHead(): Promise<string> {
  const fs: (() => number)[] = [];
  for (const k = 5, g = (x = k) => x; fs.length < 3; ) {
    await null;
    fs.push(() => k + g());
  }
  return fs.map((h) => h()).join(",");
}

async function main(): Promise<void> {
  log.push("updateNoLet " + (await updateNoLet()));
  log.push("updateBody " + (await updateBody()));
  log.push("initReadsNoLet " + (await initReadsNoLet()));
  log.push("constHead " + (await constHead()));
}
await main();
for (const line of log) console.log(line);
