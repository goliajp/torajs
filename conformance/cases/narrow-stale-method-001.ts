// a narrowed binding a call set to null, handed back by a class method
// whose return type is not nullable: the value passes along as it is,
// and the member read off the call result throws a TypeError
class Node {
  n: number;
  constructor(n: number) { this.n = n }
}
let q: Node | null = new Node(2);
function nullQ(): boolean { q = null; return true }
function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
  q = new Node(2);
}

class Box {
  k: number;
  constructor(k: number) { this.k = k }
  keep(x: Node): Node { return x }
  fresh(): Node { return new Node(this.k + 1) }
}

// one owner of the method: the call is static
const b = new Box(3);
run("method", () => { if (q !== null && nullQ()) console.log(b.keep(q).n) });
run("new", () => { if (q !== null && nullQ()) console.log(new Box(4).keep(q).n) });
run("chain", () => { if (q !== null && nullQ()) console.log(b.fresh().n + b.keep(q).n) });

// an override: the call dispatches over both owners
class Shape {
  w: number;
  constructor(w: number) { this.w = w }
  pick(x: Node): Node { return new Node(this.w + x.n) }
}
class Square extends Shape {
  pick(x: Node): Node { return x }
}
function viaBase(s: Shape, x: Node): number { return s.pick(x).n }
run("override", () => { if (q !== null && nullQ()) console.log(viaBase(new Square(5), q)) });
run("base", () => { if (q !== null && nullQ()) console.log(viaBase(new Shape(5), new Node(1))) });

// two unrelated classes declare the method: the receiver's static type
// picks the owner
class Left {
  hold(x: Node): Node { return x }
}
class Right {
  hold(x: Node): Node { return new Node(x.n + 1) }
}
const l = new Left();
const r = new Right();
run("sibling", () => { if (q !== null && nullQ()) console.log(l.hold(q).n) });
run("sibling2", () => { if (q !== null && nullQ()) console.log(r.hold(new Node(9)).n + l.hold(q).n) });

// passed along without a member read, the value is null
run("pass", () => { if (q !== null && nullQ()) { const z = b.keep(q); console.log(z === null, z, typeof z) } });

// the live path still reads
const live = new Node(7);
console.log(b.keep(live).n, b.fresh().n, new Square(6).pick(live).n, viaBase(new Shape(8), live), l.hold(live).n, r.hold(live).n);
