// An await in the test or the update of a `for (let …)` head whose
// update makes closures (§14.7.4.4): the test and the update run on the
// iteration's copy across the await, so each closure sees the value the
// increment that follows it gave.
const tick = async (i: number): Promise<boolean> => i < 3;
const next = async (i: number): Promise<number> => i + 1;

async function awaitInTest(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0; await tick(i); fs.push(() => i), i++) {}
  return fs.map((h) => h()).join(",");
}

async function awaitInUpdate(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0; i < 3; i = await next(i), fs.push(() => i)) {}
  return fs.map((h) => h()).join(",");
}

async function awaitInBoth(): Promise<string> {
  const fs: (() => number)[] = [];
  for (let i = 0; await tick(i); fs.push(() => i), i = await next(i)) fs.push(() => i * 10);
  return fs.map((h) => h()).join(",");
}

async function main(): Promise<void> {
  console.log("awaitInTest", await awaitInTest());
  console.log("awaitInUpdate", await awaitInUpdate());
  console.log("awaitInBoth", await awaitInBoth());
}
main();
