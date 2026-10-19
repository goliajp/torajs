// An async function waiting on a promise nothing will ever settle is
// abandoned once the last reference to that promise goes: its frame is
// released with what it held (a string, an object), its own promise
// stays pending, and the program ends normally. Many such frames in a
// loop leave nothing behind.

class Held {
  constructor(public tag: string) {}
}

let started = 0;
let finished = 0;

async function waitForever(i: number): Promise<number> {
  const s = "held-" + i;
  const h = new Held(s);
  started++;
  await new Promise<number>(() => {});
  finished++;
  return h.tag.length + s.length;
}

async function waitSettled(i: number): Promise<string> {
  const s = "kept-" + i;
  await null;
  return s;
}

async function main(): Promise<void> {
  for (let i = 0; i < 100000; i++) {
    waitForever(i);
  }
  const results: string[] = [];
  for (let i = 0; i < 3; i++) {
    results.push(await waitSettled(i));
  }
  console.log(started, finished, results.join(","));
}

main();
