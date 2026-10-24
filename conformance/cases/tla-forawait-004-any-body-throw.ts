// A top-level `for await` over values the checker types `any`, left by
// a throw out of its body, against a `.then` chain: AsyncIteratorClose
// (§7.4.11) still calls `return()` and awaits what it answers before
// the throw goes on, and whatever the close itself raises (a rejected
// answer, a throwing `return()`, an answer that is no object) is
// dropped for the original throw (step 6). Over a sync iterator the
// close is the Async-from-Sync wrapper's `return` (§27.1.6.2.2): two
// jobs past the settling of the answer's `value`, one when it has no
// `return` or rejects.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(60);

function asyncIter(label: string, ret: () => any): any {
  return {
    [Symbol.asyncIterator]() {
      return {
        next() {
          return Promise.resolve({ value: 1, done: false });
        },
        return() {
          log.push(label + " return");
          return ret();
        },
      };
    },
  };
}

function syncIter(label: string, ret: () => any): any {
  return {
    [Symbol.iterator]() {
      return {
        next() {
          return { value: 1, done: false };
        },
        return() {
          log.push(label + " return");
          return ret();
        },
      };
    },
  };
}

async function* gen() {
  try {
    yield 1;
  } finally {
    await null;
    log.push("gen finally");
  }
}

const sources: [string, any][] = [
  ["gen", gen()],
  ["fulfilled", asyncIter("fulfilled", () => Promise.resolve({ done: true }))],
  ["rejected", asyncIter("rejected", () => Promise.reject(new Error("close")))],
  [
    "throwing",
    asyncIter("throwing", () => {
      throw new Error("close");
    }),
  ],
  ["primitive", asyncIter("primitive", () => 5)],
  ["sync", syncIter("sync", () => ({ value: Promise.resolve(7), done: true }))],
  ["sync-primitive", syncIter("sync-primitive", () => 5)],
  [
    "sync-throwing",
    syncIter("sync-throwing", () => {
      throw new Error("close");
    }),
  ],
  ["array", [1, 2]],
];

for (const [label, src] of sources) {
  try {
    for await (const v of src) {
      log.push(label + " body " + v);
      throw new Error(label);
    }
  } catch (e: any) {
    log.push("caught " + e.message);
  }
}
log.push("done");
await null;
await null;
for (const line of log) console.log(line);
