// P8.1 — private class fields via `#priv` (ES2022 §6.2.10 PrivateName).
// Covers: declaration, in-method read via `this.#x`, in-method write
// via `this.#x = ...`, and method-to-method use through the public
// surface. Cross-class enforcement (subclass cannot access parent's
// `#priv`, foreign class cannot access via typed receiver) is a
// typecheck negative case verified out-of-band — both bun and tr
// reject those, with different error wording.

class Counter {
  #count: number = 0
  inc(n: number): number {
    this.#count = this.#count + n
    return this.#count
  }
  get(): number {
    return this.#count
  }
  reset(): void {
    this.#count = 0
  }
}

const c = new Counter()
console.log(c.inc(5))
console.log(c.inc(3))
console.log(c.get())
c.reset()
console.log(c.get())
console.log(c.inc(10))
