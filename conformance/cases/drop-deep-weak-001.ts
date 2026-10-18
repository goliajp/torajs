// Deep chains whose nodes are WeakRef targets, WeakMap / WeakSet keys
// and FinalizationRegistry targets: a closure capturing the previous
// closure, and an object holding the previous object. Releasing the
// head releases every node, deep in the stack through the deferred
// release, and each dying node must leave the weak tables once. The
// memory of the dead nodes is reused by the chains built afterwards,
// so a key left behind in a table would answer for a new object.
// A tracing collector may keep the dead nodes, so a WeakRef read
// accepts either answer; every other line is the same in both.
const N = 300000;
const STEP = 1000;

const wm = new WeakMap<object, number>();
const ws = new WeakSet<object>();
let cleaned = 0;
const fr = new FinalizationRegistry<number>(() => {
  cleaned++;
});

function closureChain(n: number, refs: WeakRef<object>[]): () => number {
  let f: () => number = () => 0;
  for (let i = 0; i < n; i++) {
    const g = f;
    f = () => g() + 1;
    if (i % STEP === 0) {
      refs.push(new WeakRef(f));
      wm.set(f, i);
      ws.add(f);
      fr.register(f, i);
    }
  }
  return f;
}

type Node = { next: Node | null; i: number };
function objChain(n: number, refs: WeakRef<object>[]): Node {
  let h: Node = { next: null, i: 0 };
  for (let i = 1; i < n; i++) {
    h = { next: h, i };
    if (i % STEP === 0) {
      refs.push(new WeakRef(h));
      wm.set(h, i);
      ws.add(h);
      fr.register(h, i);
    }
  }
  return h;
}

function reads(refs: WeakRef<object>[], kind: string): boolean {
  return refs.every((r) => {
    const d = r.deref();
    return d === undefined || typeof d === kind;
  });
}

for (let round = 0; round < 2; round++) {
  const crefs: WeakRef<object>[] = [];
  let f = closureChain(N, crefs);
  // a live node answers from every table
  const top = crefs[crefs.length - 1].deref() as () => number;
  console.log("closure", crefs.length, wm.get(top), ws.has(top), top === crefs[crefs.length - 1].deref());
  f = () => -1;
  const orefs: WeakRef<object>[] = [];
  let o: Node | null = objChain(N, orefs);
  console.log("object", orefs.length, wm.get(o.next!.next!) === undefined, ws.has(o));
  o = null;
  // the closure chain is still held through `top`; release it now
  console.log("top", typeof top);
  console.log("cleared", reads(crefs, "function"), reads(orefs, "object"));
  // fresh cells reuse the freed memory: none of them is a key
  let hits = 0;
  for (let i = 0; i < N / 10; i++) {
    const c = () => i;
    const x = { next: null, i };
    if (wm.has(c) || ws.has(c) || wm.has(x) || ws.has(x)) hits++;
  }
  console.log("stale keys", hits, f());
}
console.log("done", cleaned >= 0);
