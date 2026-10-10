// The copy an object cast makes keeps the shape of what it copies: an
// object reached twice (a cycle, or one value under two names) becomes
// one copy, and a getter that throws stops the cast with its own
// exception. The copy is an instance of the slot's class, which bun
// does not make, so this case is checked against its .expected.

class Node {
  v: number;
  next: Node | null;
  constructor(v: number) {
    this.v = v;
    this.next = null;
  }
}

const o: any = JSON.parse('{"v":1,"next":null}');
o.next = o;
const n: Node = o;
console.log(n.v, n.next === n, n.next!.next === n, n instanceof Node);

class Pair {
  a: Node;
  b: Node;
  constructor(a: Node, b: Node) {
    this.a = a;
    this.b = b;
  }
}
const shared: any = { v: 2, next: null };
const pairSrc: any = { a: shared, b: shared };
const p: Pair = pairSrc;
console.log(p.a === p.b, p.a.v);

const g: any = {
  get v(): number {
    throw new RangeError("boom");
  },
  next: null,
};
try {
  const m: Node = g;
  console.log(m.v);
} catch (e) {
  console.log(e instanceof RangeError, (e as Error).message);
}

class Box {
  items: Box[];
  constructor() {
    this.items = [];
  }
}
const b: any = { items: [] };
b.items.push(b);
const bx: Box = b;
console.log(bx.items.length, bx.items[0] === bx);

class Bag {
  items: Node[];
  tag: string;
  constructor() {
    this.items = [];
    this.tag = "";
  }
}
try {
  const bag: Bag = JSON.parse('{"items":[{"v":1,"next":null}],"tag":5}');
  console.log(bag.tag);
} catch (e) {
  console.log(e instanceof TypeError);
}
