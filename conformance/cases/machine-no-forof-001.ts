// state machines without a for-of loop: generators, async functions
// and async generators whose only loops are while / for / for-in and
// a yield*, next to a plain function that does iterate with for-of

function* counter(n: number): Generator<number> {
  let i = 0;
  while (i < n) {
    yield i;
    i++;
  }
}

function* keys(o: { [k: string]: number }): Generator<string> {
  for (const k in o) yield k + ":" + o[k];
}

function* delegate(): Generator<number> {
  yield 100;
  yield* counter(3);
  yield 200;
}

async function addLater(a: number, b: number): Promise<number> {
  const x = await Promise.resolve(a);
  const y = await Promise.resolve(b);
  return x + y;
}

async function loopAwait(n: number): Promise<number> {
  let t = 0;
  for (let i = 0; i < n; i++) t += await Promise.resolve(i);
  return t;
}

async function* ticks(n: number): AsyncGenerator<number> {
  let i = 0;
  while (i < n) {
    yield await Promise.resolve(i * i);
    i++;
  }
}

async function* relay(): AsyncGenerator<number> {
  yield* ticks(3);
}

function plainSum(xs: number[]): number {
  let t = 0;
  for (const x of xs) t += x;
  return t;
}

async function main(): Promise<void> {
  console.log([...counter(4)].join(" "));
  console.log([...keys({ a: 1, b: 2 })].join(" "));
  console.log([...delegate()].join(" "));
  console.log(await addLater(2, 3), await loopAwait(5));
  const out: number[] = [];
  const it = relay();
  let r = await it.next();
  while (!r.done) {
    out.push(r.value);
    r = await it.next();
  }
  console.log(out.join(" "), plainSum([1, 2, 3]));
}

main();
