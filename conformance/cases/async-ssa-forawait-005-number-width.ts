// A `for await` in an async function over a class async iterator, a Set
// and a sync iterable object hands the body numbers at their own width:
// a fractional step value reaches an integer-looking array and a sum
// whole, with no generator anywhere in the program to widen the shared
// step class.
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
class Quarters {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next(): { value: number; done: boolean } {
        i = i + 1;
        return { value: i / 4, done: i > 3 };
      },
    };
  }
}
async function main(): Promise<void> {
  const out: number[] = [];
  for await (const v of new AHalves()) {
    out.push(v);
  }
  let sum = 0;
  for await (const q of new Quarters()) {
    sum += q;
  }
  const ns: number[] = [];
  for await (const x of new Set([1, 2.25, 3])) {
    ns.push(x * 2);
  }
  console.log(out.join(","), sum, ns.join(","));
}
main();
