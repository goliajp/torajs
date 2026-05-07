// T-19.h (v0.5.0) — arrow expr-body return-type inference. The
// shape `(v: number) => v + 1` is JS-spec shorthand for
// `(v: number) => v + 1` returning `number`. Before this, every
// un-annotated arrow expr-body got `: void` from the desugar pass
// and the call site rejected its result with
// `expected Function([Number], Number), got Function([Number], Void)`.
//
// Covered: non-capturing arrow + map / filter / reduce surfaces.
// Capturing-arrow expr-body (`(v) => v + capture`) deferred — needs
// capture-aware return inference, gates on T-15.g.5 substrate.

let xs: number[] = []
for (let i = 0; i < 5; i = i + 1) {
  xs.push(i)
}

let doubled = xs.map((v: number) => v * 2)
console.log(doubled[0], doubled[1], doubled[2], doubled[3], doubled[4])  // 0 2 4 6 8

let evens = xs.filter((v: number) => v % 2 === 0)
console.log(evens.length)  // 3 (0, 2, 4)

// xs.map cb whose body returns string / boolean is deferred — Array
// .map MVP is homogeneous (T → T) until generic methods land. Filter
// suffices to demonstrate non-Number inferred ret (cb returns boolean).
