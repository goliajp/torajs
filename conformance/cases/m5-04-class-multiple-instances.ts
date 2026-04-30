class Counter {
  n: number;
  constructor(start: number) { this.n = start; }
  inc(): void { this.n = this.n + 1; }
  get(): number { return this.n; }
}
let a = new Counter(0);
let b = new Counter(100);
a.inc();
a.inc();
b.inc();
console.log(a.get());
console.log(b.get());
