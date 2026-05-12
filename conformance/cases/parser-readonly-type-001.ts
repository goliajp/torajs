// V3-18 wedge — TS `readonly T[]` modifier in type annotation
// position. Per TS spec §3.10.2, `readonly` on an array-of
// type marks it as a non-mutating view (typeside only, no
// runtime effect). Pre-fix tora's parser bailed with 'expected
// `,` or `)`, got Ident("number")' because `readonly` was
// parsed as a type name itself.
//
// Subset: parser skips the `readonly` modifier when followed
// by another type-ann-shape token (Ident / `void` / `{` / `(`).
// The mutability constraint is not yet enforced — calling
// `.push()` on a `readonly number[]` typed binding still
// works at runtime. Common TS use case is fn parameters.
//
// Subset limitation: `type RNS = readonly string[]` (a bare
// type-ann RHS) is not yet supported — `type =` still requires
// a `{ ... }` body in this subset. Use `readonly` directly at
// the param / var-decl site instead.

function sum(xs: readonly number[]): number {
  let s = 0
  for (let x of xs) s += x
  return s
}
console.log(sum([1, 2, 3]))            // 6

// Variable declaration with readonly modifier.
const arr: readonly number[] = [10, 20, 30]
console.log(arr.length)                // 3
console.log(arr[1])                    // 20

// readonly inside inline-obj field type.
type Box = { values: readonly number[] }
let b: Box = { values: [100, 200] }
console.log(b.values.length)           // 2
console.log(b.values[0])               // 100
