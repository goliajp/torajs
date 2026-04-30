class Counter {
  count: number;
  constructor(n: number) { this.count = n; }
  inc(): void { this.count = this.count + 1; }
  get(): number { return this.count; }
}
let c = new Counter(0);
c.inc();
c.inc();
c.inc();
console.log(c.get());
