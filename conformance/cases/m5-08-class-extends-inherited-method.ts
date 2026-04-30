class Base {
  v: number;
  constructor(n: number) { this.v = n; }
  doubled(): number { return this.v + this.v; }
}
class Derived extends Base {
  extra: number;
  constructor(n: number) {
    super(n);
    this.extra = 10;
  }
  combined(): number { return this.doubled() + this.extra; }
}
let d = new Derived(5);
console.log(d.doubled());
console.log(d.combined());
