// rotation 191 — `Array<T>.reduce(cb, seed)` where cb's
// accumulator U differs from the element T. Same shape as
// arr-map-heterogeneous-return-001 but for reduce: the
// method-table entry declared reduce as `((T, T) => T, T) => T`
// and `infer_anonymous_closure_params` strapped both cb params
// AND the return to elem_ann; the concrete
// `[1,2,3].reduce((a: string, x: number) => a + x, '')`
// tripped `check_stmt_return` on the arrow body's Str return,
// AND (once the return was un-strapped) the cb-sig check
// rejected `(String, Number) => String` vs expected
// `(Number, Number) => Number`.
//
// Fix mirrors the map arm:
// - `infer_closure_params` reduces routes through
//   `param_only_updates`; the acc param is seeded from the seed
//   arg's static type (args[1] literal / typed ident), falls
//   back to elem_ann for the sum/max idiom.
// - Call-site wedge `check_type_of_call_arr_reduce_hetero`
//   admits `(U, T) => R` for primitive `U ∈ {Number, String,
//   Boolean, Any}`, validates seed against acc, answers `R`
//   (the cb's actual return).

// Number → String (the P0 probe target)
const xs: number[] = [1, 2, 3]
console.log('concat:', xs.reduce((acc: string, x: number) => acc + x, ''))

// homogeneous (T, T) => T keeps the method-table arm
console.log('sum:', xs.reduce((a, b) => a + b, 0))

// Number → Boolean (acc = Boolean, ret = Boolean)
console.log('all-pos:', xs.reduce((a: boolean, x: number) => a && x > 0, true))

// Number → String, cb returns a formatted line
console.log('lines:', xs.reduce((a: string, x: number) => a + '[' + x + ']', ''))

// String[] → Number acc (count total length)
const ws: string[] = ['aa', 'bbb', 'c']
console.log('tot-len:', ws.reduce((a: number, w: string) => a + w.length, 0))
