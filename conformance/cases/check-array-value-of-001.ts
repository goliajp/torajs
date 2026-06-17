// ES §23.1.3.34 — `arr.valueOf()` returns the Array itself (identity).
// Array doesn't override the default Object valueOf, so the call is a
// no-op at runtime. Pre-fix tr rejected the call at the type checker
// ("not callable: type Any" — the bare member lookup fell through to
// Any). Fix wires a typed-Array arm that returns `Array<T>` so the
// call typechecks, and ssa_lower folds the call to the receiver
// operand without a runtime helper.

const ns: number[] = [1, 2, 3]
console.log(ns.valueOf().length)         // 3
console.log(ns.valueOf()[1])             // 2
console.log(ns.valueOf() === ns)         // true (identity, same heap object)

const ss: string[] = ["a", "b"]
console.log(ss.valueOf().length)         // 2
console.log(ss.valueOf()[0])             // "a"

const bs: boolean[] = [true, false, true]
console.log(bs.valueOf()[0])             // true

// Reference equality on two distinct array literals — two different
// heap allocations, so identity is false. Documents the non-pooled
// behaviour expected from the runtime.
console.log([1, 2, 3].valueOf() === [1, 2, 3].valueOf())  // false

// Method chain on `.valueOf()` result keeps the same Array<T> face.
console.log([1, 2, 3].valueOf().map((x: number) => x * 2))  // [2, 4, 6]
console.log([1, 2, 3].valueOf().reduce((a: number, b: number) => a + b, 0))  // 6
