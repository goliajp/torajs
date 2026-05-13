// V3-18 wedge — `Array<T>` / `ReadonlyArray<T>` / `Iterable<T>`
// generic shorthand for `T[]`. TS users write both forms
// interchangeably; the spec treats `Array<T>` as the canonical
// Library form. Pre-fix tora's parser produced the flat
// `Array<number>` ann string, but neither check.rs nor
// ssa_lower had a mapping for it — `let xs: Array<number>`
// failed at typecheck with 'unknown type Array<number>'.
//
// Implementation: mirror handlers in
// check.rs::resolve_type_ann_full and ssa_lower::parse_type
// recognize `Array<T>` / `ReadonlyArray<T>` / `Iterable<T>`
// (single arg only) and recursively resolve the inner T,
// wrapping in Type::Array. The two sites are kept in lockstep
// so SSA + check always agree on the lowered shape.
//
// ReadonlyArray is identity-mapped (the immutability marker
// has no runtime effect in the subset). Iterable<T> resolves
// to Array<T> for typecheck purposes — the for-of source path
// already accepts arrays as iterables, and Iterable is the
// most common annotation users reach for when documenting
// "this fn accepts any iterable, in practice an array".
//
// Subset limitation: nested `Array<Array<T>>` parses as
// `>>` (one ShrShr token) by the lexer and is rejected by
// type-arg parser — same constraint that applies to all
// generic instantiations today. Workaround: write `T[][]`
// or use a single-level alias.

let xs: Array<number> = [1, 2, 3]
console.log(xs)                        // [ 1, 2, 3 ]
console.log(xs.length)                 // 3

let ys: ReadonlyArray<number> = [4, 5, 6]
console.log(ys)                        // [ 4, 5, 6 ]

let strs: Array<string> = ["a", "b", "c"]
console.log(strs)                      // [ "a", "b", "c" ]

let bools: Array<boolean> = [true, false, true]
console.log(bools)                     // [ true, false, true ]

// Method chains still work (the underlying type is Array<T>).
let xs2: Array<number> = [1, 2, 3, 4, 5]
console.log(xs2.filter((x: number) => x > 2))
                                       // [ 3, 4, 5 ]
console.log(xs2.map((x: number) => x * 10))
                                       // [ 10, 20, 30, 40, 50 ]

// Function return type.
function ones(n: number): Array<number> {
  let r: number[] = []
  for (let i = 0; i < n; i++) r.push(1)
  return r
}
console.log(ones(4))                   // [ 1, 1, 1, 1 ]

// Function param type.
function sum(xs: Array<number>): number {
  let s = 0
  for (let x of xs) s += x
  return s
}
console.log(sum([10, 20, 30]))         // 60

// Iterable<T> resolves to Array<T> in the subset.
function take2(it: Iterable<string>): string {
  let i = 0
  let r = ""
  for (let s of it) {
    r += s
    i++
    if (i === 2) break
  }
  return r
}
console.log(take2(["a", "b", "c"]))    // ab
