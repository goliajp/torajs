// The close a `break` out of a top-level `for await` over an `any`
// sync iterator awaits, against a `.then` chain: the Async-from-Sync
// wrapper's `return` (§27.1.6.2.2) is fulfilled from a reaction on
// PromiseResolve of the answer's `value`, so a value that settles later
// delays the loop's exit past it; a `return()` that throws or answers
// no object rejects the wrapper's promise, raised one job later; an
// iterator helper's own `return` answers `{ value: undefined, done:
// true }`, two jobs.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(50);

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

const sources: [string, any][] = [
  [
    "late value",
    syncIter("late value", () => ({
      value: Promise.resolve(0)
        .then((x) => x)
        .then((x) => x),
      done: true,
    })),
  ],
  [
    "throwing",
    syncIter("throwing", () => {
      throw new Error("close");
    }),
  ],
  ["primitive", syncIter("primitive", () => null)],
  ["helper", [1, 2, 3].values().map((x: number) => x * 2)],
];
for (const [label, src] of sources) {
  try {
    for await (const v of src) {
      log.push(label + " body " + v);
      break;
    }
    log.push(label + " after");
  } catch (e: any) {
    log.push(label + " caught " + e.constructor.name);
  }
}
log.push("done");
await null;
for (const line of log) console.log(line);
