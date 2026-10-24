// A top-level `for await` over values the checker types `any`, step by
// step against a `.then` chain: an array of promises and plain values
// and a string (sync lanes under the Async-from-Sync iterator: two jobs
// a step, the exhausted step too), an async generator and a user async
// iterator (one job after the promise `next()` answers settles), and
// the close a `break` awaits (§7.4.11): an async iterator's `return()`
// answer, the wrapper's one job over a sync iterator without `return`,
// two over one whose `return()` answers an object.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(80);

const mixed: any = [Promise.resolve(1), 2, Promise.resolve(3)];
for await (const v of mixed) log.push("mixed " + v);

const word: any = "ab";
for await (const c of word) log.push("char " + c);

async function* gen() {
  yield 1;
  yield 2;
  yield 3;
}
const g: any = gen();
for await (const v of g) log.push("gen " + v);

let i = 0;
const asyncIter: any = {
  [Symbol.asyncIterator]() {
    return this;
  },
  next() {
    i++;
    log.push("next " + i);
    return Promise.resolve({ value: i, done: i > 4 });
  },
  return() {
    log.push("return");
    return Promise.resolve({ value: 0, done: true });
  },
};
for await (const v of asyncIter) {
  log.push("async " + v);
  if (v === 2) break;
}

const plainArr: any = [5, 6, 7];
for await (const v of plainArr) {
  log.push("plain " + v);
  if (v === 6) break;
}

let j = 0;
const syncIter: any = {
  [Symbol.iterator]() {
    return this;
  },
  next() {
    j++;
    return { value: j, done: j > 4 };
  },
  return() {
    log.push("sync return");
    return { done: true };
  },
};
for await (const v of syncIter) {
  log.push("sync " + v);
  if (v === 2) break;
}
log.push("done");
await null;
await null;
for (const line of log) console.log(line);
