// Loop accumulators that start as '' and leave their frame: returned,
// pushed into an array, captured by a closure, alive across an await
// or a yield.
const z = Number(process.argv.length > 5 ? 1 : 0);

function returned(n: number): string {
  let a = '';
  for (let i = 0; i < n; i++) a += '中';
  return a;
}
const r0 = returned(z);
const r3 = returned(z + 3);
console.log(r0 === '', r0.length, 'b' + r0 === 'b', '<b>'.includes('b' + r0), r3, r3.length);

function pushed(n: number, out: string[]): void {
  let a = '';
  for (let i = 0; i < n; i++) a += i % 3 ? 'é' : 'x';
  out.push(a);
}
const arr: string[] = [];
for (let k = 0; k < 4; k++) pushed(z + k * 5, arr);
console.log(arr.length, arr.join('|'), arr[0] === '');

function captured(n: number): () => string {
  let a = '';
  for (let i = 0; i < n; i++) a += 'xy';
  return () => a + '!';
}
const c0 = captured(z);
const c2 = captured(z + 2);
console.log(c0(), c2(), c2().length);

async function acrossAwait(n: number): Promise<number> {
  let a = '';
  for (let i = 0; i < n; i++) {
    a += 'q';
    await null;
  }
  await Promise.resolve();
  return a.length;
}

async function afterAwait(n: number): Promise<string> {
  let a = '';
  for (let i = 0; i < n; i++) a += '中';
  await Promise.resolve();
  return 'b' + a;
}

function* yielding(n: number): Generator<number> {
  let a = '';
  for (let i = 0; i < n; i++) {
    a += 'w';
    yield a.length;
  }
  yield a.length * 10;
}
console.log([...yielding(z + 3)].join(','), [...yielding(z)].join(','));

async function main(): Promise<void> {
  console.log(await acrossAwait(z + 4), await acrossAwait(z));
  const s = await afterAwait(z);
  console.log(s, s === 'b', '<b>'.includes(s), (await afterAwait(z + 2)).length);
}
main();
