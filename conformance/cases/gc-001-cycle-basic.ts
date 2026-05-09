// V3-08 — Bacon-Rajan cycle collector smoke test (manual trigger).
// Two-class A↔B mutual reference cycle. Without a cycle collector
// this leaks at process exit (refcounts stay at 1 each, but the
// objects are unreachable). With it, `Bun.gc(true)` walks the
// buffered roots and frees the cycle.
//
// User code can't directly observe heap freeing — what we verify
// here is that the surface compiles, runs, and the gc() call
// doesn't crash on a real cycle. Substrate (cycle_buffer +
// trial-deletion mark/scan/collect) shipped with c76b3a3 (T-26.C);
// V3-08 wires `Bun.gc(true)` through to `__torajs_cycle_collect`.

class A {
  v: number;
  b: B | null;
  constructor(v: number) { this.v = v; this.b = null; }
}
class B {
  v: number;
  a: A | null;
  constructor(v: number) { this.v = v; this.a = null; }
}

let a = new A(1)
let b = new B(2)
a.b = b
b.a = a

console.log(a.v)
console.log(b.v)
console.log(a.b === null)
console.log(b.a === null)

Bun.gc(true)
console.log('after gc')
