// An async generator reads its parameter after it has yielded and the
// call that made it has returned; the argument was read out of a field
// the body then clears. The generator object keeps the value. `churn`
// reallocates cells of the same size, so a freed one would answer with
// another value. (Apart from alias-022: a program with an async
// generator function does not yet compile async class methods.)

class P {
  v: number;
  w: number;
  constructor(v: number) {
    this.v = v;
    this.w = v * 2;
  }
}
class Box {
  p: P | null = null;
}
const box = new Box();
const keep: P[] = [];
function churn(): void {
  for (let i = 0; i < 4; i++) keep.push(new P(900 + i));
}
async function* agen(q: P): AsyncGenerator<number> {
  yield q.v;
  box.p = null;
  churn();
  yield q.w;
}
async function main(): Promise<void> {
  box.p = new P(9);
  for await (const x of agen(box.p!)) console.log("async gen", x);
}
main();
