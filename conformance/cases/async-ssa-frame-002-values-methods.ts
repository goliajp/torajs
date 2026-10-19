// Async arrow values and async methods suspend over a frame like a
// declaration: the receiver and the captured bindings are read again
// after each await, and a capture the body changes keeps its new value.

class Counter {
  count = 0;
  constructor(public name: string) {}
  async bump(by: number): Promise<number> {
    const before = this.count;
    await null;
    this.count = before + by;
    await Promise.resolve(0);
    return this.count;
  }
  static async make(name: string): Promise<Counter> {
    await null;
    return new Counter(name);
  }
  async label(): Promise<string> {
    const n = await this.bump(1);
    return this.name + "#" + n;
  }
}

async function main(): Promise<void> {
  let seen = 0;
  const prefix = "v";
  const add = async (x: number): Promise<number> => {
    await null;
    seen += x;
    return x + seen;
  };
  const tag = async (s: string): Promise<string> => {
    const t = prefix + s;
    await Promise.resolve(1);
    return t + seen;
  };
  console.log(await add(2), await add(3), seen);
  console.log(await tag("a"));

  const c = await Counter.make("c1");
  console.log(await c.bump(5), await c.bump(2), c.count);
  console.log(await c.label());

  const results: number[] = [];
  const work = async (i: number): Promise<void> => {
    await null;
    results.push(i * 10 + seen);
  };
  await Promise.all([work(1), work(2), work(3)]);
  console.log(results.join(","));
}

main();
