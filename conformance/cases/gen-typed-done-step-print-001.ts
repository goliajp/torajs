// A typed async generator's done step, printed whole: its `value`
// slot holds the F64 spelling of `undefined`, and the struct printer
// reads it back as `undefined` — it used to box the raw bits and
// print `value: NaN`. (A typed read of the same slot already answered
// `undefined`; only the reflective walk was wrong.)
async function* g(): AsyncGenerator<number> { yield 1.5; }
async function* h(n: number): AsyncGenerator<number> { yield n * 2; return; }
class Box { n: number; constructor(n: number) { this.n = n; } }
async function main() {
  const it = g();
  console.log(await it.next());
  console.log(await it.next());
  console.log(await it.next());
  const jt = h(4);
  let s = await jt.next();
  console.log(s, s.value, s.done);
  s = await jt.next();
  console.log(s, s.value, s.done);
  const xs: number[] = [1.5, 2.5];
  console.log(new Box(xs[0]), new Box(xs[5]));
}
main();
