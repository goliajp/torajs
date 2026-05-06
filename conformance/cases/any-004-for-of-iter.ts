// T-10.d.iii (v0.4.0) — for-of iteration over Array<Any>. The
// existing parse-time desugar rewrites `for (let v of xs)` to a
// classic for-i loop + `let v = xs[__i]`, so the Type::Any boxed
// indexed-read path from T-10.d.i carries the body unchanged.
// `let v` infers Type::Any from the init's operand type; console.log
// dispatches via the print_any path.

let xs: any[] = [42, 'hi', true, 3.14]
for (let v of xs) {
  console.log(v)
}

// Empty Array<Any> — loop body must not execute.
let empty: any[] = []
console.log('before-empty')
for (let v of empty) {
  console.log('SHOULD NOT PRINT')
}
console.log('after-empty')

// Larger array exercising the 2x grow path of arr_alloc_any.
let many: any[] = [1, 2, 'a', 'b', true, false, 3.5, 4.5]
let total: i64 = 0
for (let v of many) {
  total = total + 1
}
console.log(total)
