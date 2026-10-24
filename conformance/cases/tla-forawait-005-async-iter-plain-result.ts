// `for await` over an `any` value whose `@@asyncIterator` answers an
// iterator with a `next()` that returns a plain object: the async
// protocol awaits that result (§14.7.5.6 step 6.d, one job), never its
// `value` (step 6.i binds it as it is), so a promise value reaches the
// body as a promise.
const log: string[] = [];
let tick = 0;
function chain(n: number): void {
  Promise.resolve(0).then(function step(): void {
    log.push("t" + ++tick);
    if (tick < n) Promise.resolve(0).then(step);
  });
}
chain(20);

function source(): any {
  let i = 0;
  return {
    [Symbol.asyncIterator]() {
      return {
        next() {
          i++;
          return { value: Promise.resolve(i), done: i > 2 };
        },
      };
    },
  };
}

for await (const v of source()) log.push("top " + (v instanceof Promise));
log.push("done");
for (const line of log) console.log(line);
