// V3-18 wedge — TS-only assertions: `as const` and `satisfies T`.
// Both are type-only with no runtime effect. Pre-fix tora's parser
// hard-rejected with 'expected type name, got Const' / 'expected
// expression, got Colon'.
//
// `as const` — narrows a literal expression's type to its most-
// specific form. Subset treats as identity.
// `satisfies T` — TS-side type-check that the expr matches T,
// without widening the inferred type. Runtime no-op.

let s = "hello" as const
console.log(s)                        // hello

let n = 5 as const
console.log(n)                        // 5

let arr = [1, 2, 3] as const
console.log(arr.length)               // 3

let p = { a: 1, b: "hi" } satisfies { a: number; b: string }
console.log(p.a, p.b)                 // 1 hi

// Chained casts — fine.
let x = (5 as number) as const
console.log(x)                        // 5
