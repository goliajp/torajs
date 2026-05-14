// P0.2 — `typeof <Any-typed binding>` per JS spec §13.5.3.
// Pre-fix the typeof lower path panicked on Type::Any operands
// with 'not yet supported: typeof on Type::Any operand
// (lands with T-10.b)' — a holdover from the original Any-tier
// substrate plan. Now that P0.1 lands the let-init box, the
// matching typeof dispatch goes through a new runtime helper
// __torajs_any_typeof that reads the box's tag (and for
// ANY_HEAP the inner heap header's type_tag) and returns the
// spec-mandated literal: number / string / boolean / object /
// function / symbol / bigint.
//
// Implementation:
// * runtime_str.c: __torajs_any_typeof((const void *box))
//   switches on the tag, then on the heap type_tag for
//   ANY_HEAP. Allocates a fresh pooled String for the result.
// * ssa_lower: typeof on Type::Any operand routes through the
//   new helper. Other operand types (concrete) keep the
//   existing compile-time literal-string fast path.
//
// Subset notes:
// * tora has no real undefined yet (P1) so ANY_NULL collapses
//   to "object" — same as typeof null. Once P1 lands real
//   Type::Undefined the helper returns "undefined" for the
//   Undefined tag.
// * BigInt boxed in Any returns "bigint" via the heap-header
//   tag. Function (closure) boxed in Any returns "function".
//   Symbol boxed in Any returns "symbol".

let n: any = 5
console.log(typeof n)                        // number

let f: any = 3.14
console.log(typeof f)                        // number

let s: any = "hello"
console.log(typeof s)                        // string

let bt: any = true
console.log(typeof bt)                       // boolean

let bf: any = false
console.log(typeof bf)                       // boolean

let nul: any = null
console.log(typeof nul)                      // object  (typeof null === "object")

let arr: any = [1, 2, 3]
console.log(typeof arr)                      // object

// Regression — typeof on concrete-typed bindings stays on the
// compile-time literal-string fast path.
let cn: number = 42
console.log(typeof cn)                       // number

let cs: string = "x"
console.log(typeof cs)                       // string
