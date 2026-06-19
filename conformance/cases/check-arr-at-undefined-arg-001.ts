// S225 — typed `Array<T>.at(undefined)` per ES §23.1.3.1 step 2-3:
// ToIntegerOrInfinity(undefined)=0, so `xs.at(undefined)` returns
// `xs.at(0)`. Pre-fix tora rejected at the strict-arity gate with
// "argument 0: expected Number, got Undefined".

const ns: number[] = [10, 20, 30, 40]
console.log(ns.at(undefined))
console.log(ns.at(0))

const ss: string[] = ["a", "b", "c"]
console.log(ss.at(undefined))
console.log(ss.at(0))

const bs: boolean[] = [true, false, true]
console.log(bs.at(undefined))

// non-undef regression — numeric (incl. negative) index still works.
console.log(ns.at(2))
console.log(ns.at(-1))
console.log(ss.at(-2))
