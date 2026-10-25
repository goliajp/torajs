// A `for` loop in an async body whose head makes functions, against a
// `.then` chain. A closure in the initializer or the update that reads
// the head's `let` binding sees the copy of the iteration it was made
// in (§14.7.4.4 CreatePerIterationEnvironment); one that reads nothing
// of the head, or a head with only a `var` or an expression, has no
// copy to see, and the awaits around it keep their order.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(20);

async function readsBinding(): Promise<string> {
  const seen: (() => number)[] = [];
  for (let i = 0, f = () => i; i < 3; i++, seen.push(() => i)) {
    await null;
    log.push("reads " + i + " " + f());
  }
  return seen.map((g) => g()).join(",");
}

async function readsNothing(): Promise<string> {
  const made: (() => string)[] = [];
  for (let i = 0; i < 2; i++, made.push(() => "step")) {
    await null;
    log.push("nothing " + i);
  }
  return made.map((g) => g()).join(",");
}

async function varHead(): Promise<number> {
  let total = 0;
  for (var k = 0, add = (n: number) => n + 1; k < 2; k = add(k)) {
    await null;
    total += k;
    log.push("var " + k);
  }
  return total;
}

async function exprHead(): Promise<number> {
  let count = 0;
  for (
    (() => count++)();
    count < 3;
    (() => {
      count++;
    })()
  ) {
    await null;
    log.push("expr " + count);
  }
  return count;
}

async function main(): Promise<void> {
  log.push("readsBinding " + (await readsBinding()));
  log.push("readsNothing " + (await readsNothing()));
  log.push("varHead " + (await varHead()));
  log.push("exprHead " + (await exprHead()));
}
await main();
for (const line of log) console.log(line);
