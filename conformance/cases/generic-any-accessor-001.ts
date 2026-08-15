class Box<T> {
  private v: T;
  constructor(x: T) {
    this.v = x;
  }
  get value(): T {
    return this.v;
  }
  describe(): string {
    return "box:" + this.v;
  }
}

class Pair<T> {
  private v: T;
  constructor(x: T) {
    this.v = x;
  }
  get first(): T {
    return this.v;
  }
  set first(x: T) {
    this.v = x;
  }
}

class Plain {
  private w: number = 5;
  get val(): number {
    return this.w;
  }
  set val(x: number) {
    this.w = x;
  }
}

const b: any = new Box<number>(10);
console.log(b.value);
console.log(b.describe());
try {
  b.value = 99;
  console.log("no-throw");
} catch (e) {
  console.log("TE");
}

const s: any = new Box<string>("hi");
console.log(s.value);

const p: any = new Pair<number>(1);
console.log(p.first);
p.first = 42;
console.log(p.first);

const q: any = new Plain();
console.log(q.val);
q.val = 7;
console.log(q.val);
