// An `any` value appended to an array whose element annotation the
// program wrote is cast into that element type, wherever the array is
// reached from: a binding a closure captures, a class field, a field
// down a chain of class fields. A class field without an annotation
// holds its elements as `any`.

class P {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
function vals(): any[] {
  return [new P(1), new P(2)];
}

function collect(): number {
  const out: P[] = [];
  vals().forEach((v: any) => {
    out.push(v);
  });
  return out[0].x + out[1].x;
}
console.log("captured", collect());

class Bag {
  items: P[];
  constructor() {
    this.items = [];
  }
  add(a: any): void {
    this.items.push(a);
  }
  front(a: any): void {
    this.items.unshift(a);
  }
}
const b = new Bag();
for (const v of vals()) b.add(v);
b.front(new P(0));
console.log("field", b.items.length, b.items[0].x, b.items[2].x);

class Holder {
  inner: Bag = new Bag();
}
const h = new Holder();
for (const v of vals()) h.inner.items.push(v);
console.log("chain", h.inner.items.length, h.inner.items[1].x);

class Loose {
  items = [new P(1)];
  add(a: any): void {
    this.items.push(a);
  }
}
const l = new Loose();
l.add(new P(5));
l.add({ x: 2.5 });
l.add("s");
console.log("unannotated", l.items.length, l.items[1].x, l.items[2].x, l.items[3]);

class Asserted {
  items = [] as P[];
}
const s = new Asserted();
for (const v of vals()) s.items.push(v);
console.log("asserted", s.items.length, s.items[1].x);
