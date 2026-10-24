// A top-level `for await` over arrays: each step awaits the element
// (§27.1.6.4 Async-from-Sync continuation: PromiseResolve of the value,
// then its reaction fulfills the step's promise) and the loop's await
// of the step goes on one job after that reaction, so a step takes two
// jobs against a `.then` chain whether the element is a promise or
// not. Elements of number, string and promise types; a promise a later
// job settles; `break` and a throw out of the body.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(40);

const pn: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2)];
for await (const v of pn) log.push("pn " + v);

const ps: Promise<string>[] = [Promise.resolve("a"), Promise.resolve("b")];
for await (const s of ps) log.push("ps " + s + s);

const plain: number[] = [7, 8];
for await (const v of plain) log.push("plain " + v);

const words: string[] = ["x", "y"];
for await (const w of words) log.push("word " + w.toUpperCase());

let settle: (v: number) => void = () => {};
const later = new Promise<number>((r) => {
  settle = r;
});
Promise.resolve(0)
  .then(() => 0)
  .then(() => settle(30));
const mixed: Promise<number>[] = [Promise.resolve(10), later, Promise.resolve(40)];
for await (const v of mixed) {
  log.push("mixed " + v);
  if (v === 30) break;
}

try {
  for await (const v of pn) {
    log.push("throwing at " + v);
    throw new Error("out " + v);
  }
} catch (e) {
  log.push("caught " + (e as Error).message);
}

const rejected: Promise<number>[] = [Promise.resolve(1), Promise.reject(new Error("rej"))];
try {
  for await (const v of rejected) log.push("rejected " + v);
} catch (e) {
  log.push("caught " + (e as Error).message);
}
log.push("done");
await null;
await null;
for (const line of log) console.log(line);
