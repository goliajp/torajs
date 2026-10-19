// a nullable field read after a test of a discriminant field that does
// not rule out every construction leaving it null: the binding was
// written or shadowed since the test, the field was written after
// construction, the constant is NaN, or a construction passed a kind
// that is not a constant. The member read off the null throws a
// TypeError
const LEAF = 0;
const NEG = 1;
const BAD = NaN;

class Node {
  kind: number;
  n: number;
  a: Node | null;
  constructor(kind: number, n: number, a: Node | null) {
    this.kind = kind;
    this.n = n;
    this.a = a;
  }
}
function leaf(n: number): Node { return new Node(LEAF, n, null) }
function neg(a: Node): Node { return new Node(NEG, 0, a) }
function evalNode(e: Node): number {
  if (e.kind === LEAF) return e.n;
  return -evalNode(e.a!);
}

function run(label: string, t: () => void): void {
  try { t() } catch (e) { console.log(label, (e as Error).constructor.name) }
}

function rebound(): number {
  let x = neg(leaf(6));
  if (x.kind === LEAF) return 0;
  x = leaf(2);
  return x.a!.n;
}
function shadowed(): number {
  const y = neg(leaf(8));
  if (y.kind === LEAF) return 0;
  let t = y.a!.n;
  {
    const y = leaf(1);
    t += y.a!.n;
  }
  return t;
}
function caught(): number {
  const e = neg(leaf(3));
  if (e.kind === LEAF) return 0;
  try {
    throw leaf(4);
  } catch (e) {
    return (e as Node).a!.n;
  }
}
function looped(xs: Node[]): number {
  const v = neg(leaf(5));
  if (v.kind === LEAF) return 0;
  let t = 0;
  for (const v of xs) t += v.a!.n;
  return t;
}
console.log(evalNode(neg(neg(leaf(7)))));
run("rebound", () => console.log(rebound()));
run("shadowed", () => console.log(shadowed()));
run("caught", () => console.log(caught()));
run("looped", () => console.log(looped([neg(leaf(1)), leaf(2)])));

// the kind is written after construction
class Mut {
  kind: number;
  a: Mut | null;
  constructor(kind: number, a: Mut | null) {
    this.kind = kind;
    this.a = a;
  }
}
function mutRead(m: Mut): number {
  if (m.kind === LEAF) return 0;
  return m.a!.kind;
}
const m = new Mut(LEAF, null);
console.log(mutRead(new Mut(NEG, new Mut(LEAF, null))));
m.kind = NEG;
run("written", () => console.log(mutRead(m)));

// NaN equals nothing, so `kind === BAD` rules nothing out
class Odd {
  kind: number;
  a: Odd | null;
  constructor(kind: number, a: Odd | null) {
    this.kind = kind;
    this.a = a;
  }
}
function oddRead(o: Odd): number {
  if (o.kind === BAD) return 0;
  return o.a!.kind;
}
console.log(oddRead(new Odd(NEG, new Odd(LEAF, null))));
run("nan", () => console.log(oddRead(new Odd(BAD, null))));

// a construction whose kind is not a constant is never ruled out
class Var {
  kind: number;
  a: Var | null;
  constructor(kind: number, a: Var | null) {
    this.kind = kind;
    this.a = a;
  }
}
function varRead(v: Var): number {
  if (v.kind === LEAF) return 0;
  return v.a!.kind;
}
function made(k: number): Var { return new Var(k, null) }
console.log(varRead(new Var(NEG, new Var(LEAF, null))), varRead(new Var(LEAF, null)));
run("unknown kind", () => console.log(varRead(made(NEG))));
