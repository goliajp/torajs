// V3-18 wedge — Array.prototype.push / unshift accept a
// variable number of args per JS spec §22.1.3.20 / §22.1.3.34:
//   arr.push(a, b, c)        — appends a, then b, then c
//   arr.unshift(a, b, c)     — prepends so a, b, c sit at
//                              indices 0, 1, 2 (in order)
// Pre-fix tora's strict 1-arg signature rejected the multi-arg
// form with 'expected 1 argument(s), got N'.
//
// Implementation:
// * check.rs special-cases multi-arg push/unshift on Array<T>
//   when receiver is Array; enforces every arg matches the
//   element type; returns Void.
// * ast::desugar_variadic_push (NEW pass between
//   desugar_uninit_let and desugar_arguments_object): rewrites
//   `arr.push(a, b, c)` → `Stmt::Multi(arr.push(a);
//   arr.push(b); arr.push(c))`. unshift gets the args
//   reversed since sequential unshift(c), unshift(b),
//   unshift(a) is the spec-equivalent.
//
// Subset limitation: only Ident receivers are rewritten. A
// complex receiver like `o.field.push(a, b)` falls through to
// the strict-arity error since rewriting would re-evaluate
// `o.field` per call. Workaround: hoist into a temp let.

let xs: number[] = [1]
xs.push(2, 3, 4)
console.log(xs)                        // [ 1, 2, 3, 4 ]

xs.unshift(0, -1)
console.log(xs)                        // [ 0, -1, 1, 2, 3, 4 ]

// Single-arg form still works (the wedge only kicks in for N>1).
let ys: number[] = []
ys.push(99)
console.log(ys)                        // [ 99 ]

// Strings — variadic on Array<string>.
let names: string[] = ["alice"]
names.push("bob", "carol", "dave")
console.log(names.length)              // 4
console.log(names.join(","))           // alice,bob,carol,dave

// Mixed sequence with other ops.
let zs: number[] = []
zs.push(1, 2)
zs.push(3)
zs.push(4, 5, 6)
console.log(zs)                        // [ 1, 2, 3, 4, 5, 6 ]
