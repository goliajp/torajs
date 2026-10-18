// Deep chains whose every node also holds the only outside reference
// to a small cycle: releasing a node leaves its cycle alive with a
// smaller count, which makes the cycle a candidate, and the candidates
// of a deep release fill the candidate buffer many times over, so
// cycle collections run while the deferred releases of the chain are
// still pending. A pending node must survive those collections, and
// every cycle must still be reclaimed once. Object, class and closure
// chains.
const N = 300000;

type Pair = { a: any; b: any };
function pair(i: number): Pair {
  const a: any = { i };
  const b: any = { a };
  a.b = b;
  return { a, b };
}

type Node = { next: Node | null; side: any };
function objChain(n: number): Node {
  let h: Node = { next: null, side: pair(0).a };
  for (let i = 1; i < n; i++) h = { next: h, side: pair(i).a };
  return h;
}

class K {
  next: K | null;
  side: any;
  constructor(next: K | null, i: number) {
    this.next = next;
    this.side = pair(i).a;
  }
}
function classChain(n: number): K {
  let k = new K(null, 0);
  for (let i = 1; i < n; i++) k = new K(k, i);
  return k;
}

function closureChain(n: number): () => number {
  let f: () => number = () => 0;
  for (let i = 0; i < n; i++) {
    const g = f;
    const s = pair(i).a;
    f = () => g() + s.i - s.b.a.i;
  }
  return f;
}

function walk(h: Node | null): number {
  let n = 0;
  while (h !== null) {
    if (h.side.b.a !== h.side) return -1;
    n++;
    h = h.next;
  }
  return n;
}

for (let round = 0; round < 2; round++) {
  let o: Node | null = objChain(N);
  console.log("object", walk(o));
  o = null;
  let k: K | null = classChain(N);
  console.log("class", k.side.b.a === k.side, k.next!.side.i);
  k = null;
  let f: (() => number) | null = closureChain(N);
  console.log("closure", typeof f);
  f = null;
  // fresh nodes after the collections
  const t = objChain(1000);
  console.log("after", walk(t), t.side.b.a.i);
}
console.log("done");
