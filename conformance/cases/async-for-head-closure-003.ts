// A `for` in an async body whose update makes a function that reads
// the head's `let` binding only through a declaration nested in it: a
// function declaration's body, a class method, a class field
// initializer. Each function sees the copy of the
// iteration the update ran in (§14.7.4.4 CreatePerIterationEnvironment,
// then the increment in that copy), so the reads are 1, 2, 3, against
// a `.then` chain that the awaits keep their order with.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(24);

async function nestedFnBody(): Promise<string> {
  const seen: (() => number)[] = [];
  for (
    let i = 0;
    i < 3;
    seen.push(() => {
      function read(): number {
        return i;
      }
      return read();
    }),
      i++
  ) {
    await null;
    log.push("fnBody " + i);
  }
  return seen.map((g) => g()).join(",");
}

async function nestedMethod(): Promise<string> {
  const seen: (() => number)[] = [];
  for (
    let i = 0;
    i < 3;
    seen.push(() => {
      class Reader {
        read(): number {
          return i;
        }
      }
      return new Reader().read();
    }),
      i++
  ) {
    await null;
    log.push("method " + i);
  }
  return seen.map((g) => g()).join(",");
}

async function nestedField(): Promise<string> {
  const seen: (() => number)[] = [];
  for (
    let i = 0;
    i < 3;
    seen.push(() => {
      class Holder {
        v: number = i;
      }
      return new Holder().v;
    }),
      i++
  ) {
    await null;
    log.push("field " + i);
  }
  return seen.map((g) => g()).join(",");
}

async function main(): Promise<void> {
  log.push("nestedFnBody " + (await nestedFnBody()));
  log.push("nestedMethod " + (await nestedMethod()));
  log.push("nestedField " + (await nestedField()));
}
await main();
for (const line of log) console.log(line);
