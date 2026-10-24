// A top-level `for await` over an `any` sync iterator whose step
// throws: the Async-from-Sync iterator's `next` turns the throw into a
// rejected promise (§27.1.6.2 IfAbruptRejectPromise, §27.1.6.4 steps
// 1-4), so the loop raises it one job later. A throwing `next()`, a
// throwing `done` or `value` getter and a result that is no object all
// go that way; a throw out of GetIterator itself is raised at once.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(40);

function stepping(label: string, answer: (i: number) => any): any {
  let i = 0;
  return {
    [Symbol.iterator]() {
      return {
        next() {
          return answer(i++);
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
  [
    "next-throws",
    stepping("next-throws", (i) => {
      if (i === 1) throw new Error("n1");
      return { value: i, done: false };
    }),
  ],
  [
    "done-throws",
    stepping("done-throws", () => ({
      get done(): boolean {
        throw new Error("d1");
      },
      value: 0,
    })),
  ],
  [
    "value-throws",
    stepping("value-throws", () => ({
      done: false,
      get value(): number {
        throw new Error("v1");
      },
    })),
  ],
  ["not-object", stepping("not-object", () => 7)],
  [
    "get-iterator-throws",
    {
      [Symbol.iterator]() {
        throw new Error("g1");
      },
    },
  ],
  [
    "get-iterator-not-object",
    {
      [Symbol.iterator]() {
        return 5;
      },
    },
  ],
  ["helper-throws", [1, 2].values().map((x: number) => {
    if (x === 2) throw new Error("h2");
    return x;
  })],
];
for (const [label, src] of sources) {
  try {
    for await (const v of src) log.push(label + " body " + v);
    log.push(label + " finished");
  } catch (e: any) {
    log.push(label + " caught " + (e instanceof TypeError ? "TypeError" : e.message));
  }
}
log.push("done");
await null;
for (const line of log) console.log(line);
