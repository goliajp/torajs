// A top-level `for await` over an `any` sync iterator whose value is a
// rejected promise: the Async-from-Sync iterator's `next` closes the
// sync iterator before it rejects the step (§27.1.6.4
// AsyncFromSyncIteratorContinuation, closeOnRejection true), so
// `return()` runs, and the loop raises the rejection one job later. An
// array's iterator has no `return`.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(30);

function rejecting(label: string, values: any[]): any {
  let i = 0;
  return {
    [Symbol.iterator]() {
      return {
        next() {
          const value = values[i++];
          return { value, done: i > values.length };
        },
        return() {
          log.push(label + " return called");
          return {};
        },
      };
    },
  };
}

const sources: [string, any][] = [
  ["first", rejecting("first", [Promise.reject(new Error("r1"))])],
  ["second", rejecting("second", [1, Promise.reject(new Error("r2")), 3])],
  ["array", [Promise.resolve(1), Promise.reject(new Error("r3"))]],
];
for (const [label, src] of sources) {
  try {
    for await (const v of src) log.push(label + " body " + v);
    log.push(label + " finished");
  } catch (e: any) {
    log.push(label + " caught " + e.message);
  }
}
log.push("done");
await null;
for (const line of log) console.log(line);
