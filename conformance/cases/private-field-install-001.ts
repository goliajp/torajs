// RFC 20260916-private-element-brand slice 2b — a private field exists
// on an object only once its own initializer ran (§7.3.34 step 3).

function te(f: () => unknown): string {
  try {
    return "value " + String(f());
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other " + String(e);
  }
}

// an initializer reads / writes a field declared after it
class ReadLater { y = this.#x; #x = 1; }
class WriteLater { y = (this.#x = 1); #x: number = 0; }
class ReadSelf { #s: any = this.#s; }
console.log(te(() => new ReadLater()));
console.log(te(() => new WriteLater()));
console.log(te(() => new ReadSelf()));

// declaration order: earlier fields are visible to later initializers
class Order {
  #a = 1;
  pub = this.#a + 1;
  #b = this.pub + this.#a;
  #c: number = 7;
  bare;
  #d;
  sum() { return this.#a + this.#b + this.#c + String(this.#d) + this.bare; }
}
console.log(new Order().sum());

// a field no initializer precedes is there from the start
class Head {
  #h: number = 5;
  seen = this.#h;
  get h() { return this.#h + this.seen; }
}
console.log(new Head().h);

// a base constructor calling an overridden method before the derived
// class's fields exist
class Base {
  log: string[] = [];
  constructor() { this.log.push(this.probe()); }
  probe(): string { return "base"; }
}
class Derived extends Base {
  #v = 42;
  probe(): string { return te(() => this.#v); }
  get v() { return this.#v; }
}
const d = new Derived();
console.log(d.log[0], d.v);

// `this` handed out from an initializer, read back before the field exists
let leaked: any = null;
function keep(o: any) { leaked = o; return 0; }
class Leak {
  k = keep(this);
  early = te(() => leaked.peek());
  #late = "ok";
  peek() { return this.#late; }
}
const l = new Leak();
console.log(l.early, l.peek());

// a closure from an earlier initializer, called before the later field
class Arrow {
  f = () => this.#z;
  g = te(() => (0, this.f)());
  #z = 3;
  after() { return this.f(); }
}
const a = new Arrow();
console.log(a.g, a.after());

// derived: fields install after super returns, constructor body sees all
class P { base = 1; }
class Q extends P {
  #q = this.base + 1;
  #r: number;
  constructor() { super(); this.#r = this.#q * 10; }
  get qr() { return this.#q + this.#r; }
}
console.log(new Q().qr);

// every instance is independent; hot reads stay plain
class Counter { #c = 0; inc() { return ++this.#c; } }
const c1 = new Counter(), c2 = new Counter();
let total = 0;
for (let i = 0; i < 1000; i++) total += c1.inc() - c2.inc() + (i & 1 ? c2.inc() : 0);
console.log(total);
