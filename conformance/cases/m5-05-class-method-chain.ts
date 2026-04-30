class Builder {
  v: number;
  constructor() { this.v = 0; }
  add(n: number): void { this.v = this.v + n; }
  mul(n: number): void { this.v = this.v * n; }
  get(): number { return this.v; }
}
let b = new Builder();
b.add(3);
b.mul(4);
b.add(2);
console.log(b.get());
