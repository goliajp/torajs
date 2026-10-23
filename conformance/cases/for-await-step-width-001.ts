// `for await` over a class whose `[Symbol.asyncIterator]()` answers an
// object with an async `next`: the binding is the `value` of the step
// the loop awaited (§7.4.8 IteratorValue), and a fractional one reaches
// an integer-looking array at its own width
class AHalves {
  n: number = 0;
  [Symbol.asyncIterator]() {
    return this;
  }
  async next(): Promise<{ value: number; done: boolean }> {
    this.n = this.n + 1;
    return this.n <= 2 ? { value: this.n + 0.5, done: false } : { value: 0, done: true };
  }
}
async function* gen() {
  yield 1;
  yield 2.75;
}
async function main(): Promise<void> {
  const out: number[] = [];
  for await (const v of new AHalves()) {
    out.push(v);
  }
  const back: number[] = [];
  for await (const v of new AHalves()) {
    back.unshift(v * 2);
  }
  let sum = 0;
  for await (const g of gen()) {
    sum += g;
  }
  console.log(out.join(","), back.join(","), sum);
}
main();
