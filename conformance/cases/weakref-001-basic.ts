// T-26 (v0.7) — WeakRef basics. `new WeakRef(target)` observes
// `target` without keeping it alive. `wr.deref()` returns the
// target (with rc bumped) when still alive, or null after the
// target was reclaimed.
//
// The "deref returns null after reclaim" test is NOT exercised
// here because the timing of clearing is implementation-defined
// per spec — bun's V8 only clears on GC cycles, while tora's ARC
// clears synchronously on the last strong-ref drop. Both are
// conformant; only the "alive" case can be byte-compared
// against bun.

class Box {
  v: number;
  constructor(n: number) { this.v = n; }
}

// Target alive — deref returns the heap pointer.
let b = new Box(7)
let wr = new WeakRef(b)
console.log(wr.deref() === null)  // false on every conformant runtime

// typeof a WeakRef is "object" per spec.
console.log(typeof wr)

// A second WeakRef on the same target — deref returns non-null too.
let wr2 = new WeakRef(b)
console.log(wr2.deref() === null)  // false
