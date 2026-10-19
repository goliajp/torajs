// nullable fields read after tests of a discriminant field ruled out
// every construction that leaves them null: each kind of test (an early
// return, an `||` of tests, an else branch, a `continue`, a string
// constant) proves the read
const LEAF = 0;
const NEG = 1;
const ADD = 2;

class Node {
  kind: number;
  n: number;
  a: Node | null;
  b: Node | null;
  constructor(kind: number, n: number, a: Node | null, b: Node | null) {
    this.kind = kind;
    this.n = n;
    this.a = a;
    this.b = b;
  }
}

function leaf(n: number): Node { return new Node(LEAF, n, null, null) }
function neg(a: Node): Node { return new Node(NEG, 0, a, null) }
function add(a: Node, b: Node): Node { return new Node(ADD, 0, a, b) }

function evalNode(e: Node): number {
  if (e.kind === LEAF) return e.n;
  if (e.kind === NEG) return -evalNode(e.a!);
  return evalNode(e.a!) + evalNode(e.b!);
}

function sizeOf(e: Node): number {
  if (e.kind === LEAF || e.kind === NEG) {
    return e.kind === LEAF ? 1 : 1 + sizeOf(e.a!);
  } else {
    return 1 + sizeOf(e.a!) + sizeOf(e.b!);
  }
}

function firstKids(xs: Node[]): number {
  let t = 0;
  for (const x of xs) {
    if (x.kind === LEAF) continue;
    t += x.a!.n;
  }
  return t;
}

class Shape {
  tag: string;
  inner: Shape | null;
  w: number;
  constructor(tag: string, inner: Shape | null, w: number) {
    this.tag = tag;
    this.inner = inner;
    this.w = w;
  }
}

function width(s: Shape): number {
  if (s.tag == "box") return s.w;
  return 1 + width(s.inner!);
}

const tree = add(neg(leaf(3)), add(leaf(4), neg(neg(leaf(5)))));
const nested = new Shape("pad", new Shape("pad", new Shape("box", null, 4), 0), 0);
console.log(evalNode(tree), sizeOf(tree), firstKids([leaf(1), neg(leaf(2)), add(leaf(7), leaf(1))]));
console.log(width(nested), width(new Shape("box", null, 2)));
