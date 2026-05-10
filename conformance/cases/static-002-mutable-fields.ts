// V3-18 m1.h.26 — static class fields are mutable. Per JS / TS:
//   class C { static value = 0 }
//   C.value = 5      // assignment is legal
//   C.value++         // post-incr is legal
//   C.value += 10     // compound assign is legal
//
// Pre-fix: ClassDecl desugar emitted the underlying static-field
// LetDecl with `mutable: false`. ssa_lower's Assign / PostIncr
// arms then hard-rejected with "ssa-lower: assign to unknown
// ident `__sf_C__value`" (no globals slot to write to). The
// inline-literal-init read fast-path also baked in the original
// 0 at every read site, hiding writes that did succeed.
//
// Fix:
//   1. ClassDecl desugar emits `mutable: true` for static fields.
//   2. ssa_lower's globals registry only skips inline-literal-init
//      lets when `mutable == false`.
//   3. The literal-inline read fast-path also gates on mutability.
//   4. PostIncr's Ident arm checks the globals registry as a
//      fallback (matching the read / Assign paths).

class Counter {
  static value: number = 0
  static increment(): void { Counter.value++ }
  static decrement(): void { Counter.value-- }
  static add(n: number): void { Counter.value = Counter.value + n }
}

console.log(Counter.value)   // 0
Counter.increment()
Counter.increment()
Counter.increment()
console.log(Counter.value)   // 3
Counter.decrement()
console.log(Counter.value)   // 2
Counter.add(10)
console.log(Counter.value)   // 12

// Direct assign from outside a class method.
Counter.value = 100
console.log(Counter.value)   // 100
Counter.value++
console.log(Counter.value)   // 101
