// RFC 20260805 blade 3 — async METHODS suspend too. The body moves into
// a top-level generator whose leading parameter `__genrecv` is the
// receiver (the class generator-method channel), `this` in the body
// reads it, and the method itself forwards `this` and its arguments
// into the driver. Instance and static methods, a field read and a
// method call through `this`, a rejection, and bun's order: every
// method here suspends at its first await, so "sync-end" is first.
class Counter {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  step(): number {
    this.n = this.n + 1;
    return this.n;
  }
  async bump(by: number): Promise<number> {
    console.log("bump-enter", this.n);
    await Promise.resolve(0);
    this.n = this.n + by;
    console.log("bump-after", this.n);
    return this.step();
  }
  async twice(): Promise<number> {
    const a = await this.bump(1);
    const b = await this.bump(10);
    return a + b;
  }
  async fail(): Promise<number> {
    await Promise.resolve(0);
    throw new Error("nope-" + this.n);
  }
  static async make(n: number): Promise<Counter> {
    await Promise.resolve(0);
    return new Counter(n);
  }
  static async tag(x: any = "dflt") {
    await Promise.resolve(0);
    return "tag:" + x;
  }
}
const c = new Counter(0);
c.bump(5).then((v) => console.log("bump", v, c.n));
const d = new Counter(100);
d.twice().then((v) => console.log("twice", v, d.n));
c.fail().then(
  () => console.log("bad"),
  (e) => console.log("rejected", e.message)
);
Counter.make(7).then((m) => console.log("make", m.n, m.step()));
Counter.tag().then((v) => console.log("tag", v));
Counter.tag(3).then((v) => console.log("tag2", v));
console.log("sync-end");
