// An async function with a `for await` suspends its frame at every step,
// whatever else its body reads (a computed default parameter, `arguments`,
// an arrow's inherited `arguments`): each step interleaves with the jobs
// queued around it the way the spec counts them (two jobs per array step
// under the Async-from-Sync iterator, one per async iterator step).
const log: string[] = [];
function ticks(tag: string, n: number) {
  let p = Promise.resolve();
  for (let i = 1; i <= n; i++) {
    p = p.then(() => {
      log.push(tag + i);
    });
  }
  return p;
}

async function withDefault(n: number, m = n + 1) {
  const out: number[] = [];
  for await (const x of [Promise.resolve(n), m]) {
    log.push("d" + x);
    out.push(x);
  }
  return out.join("+");
}

async function withArguments(a: number, b: number) {
  let s = 0;
  for await (const x of [Promise.resolve(a), b]) {
    log.push("a" + x + "/" + arguments.length);
    s += x;
  }
  return s;
}

function outer(n: number) {
  const h = async () => {
    for await (const x of [n, Promise.resolve(n + 1)]) log.push("h" + x + "/" + arguments.length);
    return n;
  };
  return h();
}

async function anySource(src: any) {
  let k = 0;
  for await (const x of src) {
    log.push("y" + x);
    k++;
  }
  return k;
}

async function main() {
  ticks("A", 8);
  log.push("withDefault " + (await withDefault(1)));
  ticks("B", 8);
  log.push("withArguments " + (await withArguments(1, 2)));
  ticks("C", 8);
  log.push("outer " + (await outer(4)));
  ticks("D", 8);
  const src: any = [1, Promise.resolve(2)];
  log.push("anySource " + (await anySource(src)));
  await ticks("Z", 1);
  console.log(log.join(" "));
}
main();
