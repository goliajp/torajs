// V3-18 wedge — destructuring patterns in arrow-fn params per
// ES spec §14.1.3, mirror of the parse_fn wedge from 0ba77d4.
// Pre-fix the arrow-fn param parser bailed at 'expected
// parameter name, got LBracket / LBrace' the moment a `[` or
// `{` appeared in the param list.
//
// Implementation: parse_arrow_fn now routes destr patterns
// through the same parse_destr_param helper as parse_fn,
// synthesizes a fresh hidden binding name, and prepends per-
// element / per-field destructuring lets to the body before
// emitting Expr::ArrowFn.
//
// Subset limitation: closure-call type inference doesn't
// propagate the param ann from the destr position back into
// the synthetic binding. Practical effect — arrow-fn destr
// requires both:
//   * explicit `: T` on the destr pattern itself
//   * explicit `: R` return type ann on the arrow
// Without those the closure infer surface declares the param
// as Void / unknown and rejects the body. The fixture works
// only with both anns present.
//
// (Map / filter callback inference of destr-param shape is
// a separate substrate item; out of scope for this wedge.)

let f = ([a, b]: number[]): number => a + b
console.log(f([1, 2]))                         // 3
console.log(f([3, 4]))                         // 7
console.log(f([10, 20]))                       // 30

let g = ({ x, y }: { x: number, y: number }): number => x * y
console.log(g({ x: 3, y: 4 }))                 // 12
console.log(g({ x: 5, y: 6 }))                 // 30

// Rename target on object form.
let h = ({ x: u, y: v }: { x: number, y: number }): number => u - v
console.log(h({ x: 10, y: 3 }))                // 7

// Stored-arrow form.
let area = ({ w, h }: { w: number, h: number }): number => w * h
console.log(area({ w: 5, h: 4 }))              // 20

// Block-body arrow.
let stats = ([x, y]: number[]): string => {
  let s = x + y
  let p = x * y
  return "sum=" + s + " prod=" + p
}
console.log(stats([3, 4]))                     // sum=7 prod=12
