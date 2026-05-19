// P8.2 — class accessor descriptors (ES §10.1.7 [[Get]] / [[Set]]).
// Covers: getter read via `c.value`, setter write via `c.value = v`,
// round-trip through a backing private field, multi-instance
// independence, and accessor across method calls.

class Counter {
  #count: number = 0
  get value(): number {
    return this.#count
  }
  set value(n: number) {
    this.#count = n
  }
  inc(by: number): void {
    this.value = this.value + by
  }
}

const a = new Counter()
const b = new Counter()

console.log(a.value)
console.log(b.value)

a.value = 7
console.log(a.value)
console.log(b.value)

b.value = 100
a.inc(3)
console.log(a.value)
console.log(b.value)

b.value = a.value
console.log(b.value)
